//! Artifact metadata, controlled storage and scoped downloads.

use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::{
    DevRailArtifactPage, DevRailArtifactQuery, DevRailArtifactResponse, DevRailArtifactRow,
};
use crate::repositories::devrail_artifacts;
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::{Component, Path, PathBuf};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

pub struct ArtifactDownload {
    pub file_name: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

pub struct StoreArtifactInput<'a> {
    pub project_id: i64,
    pub task_id: i64,
    pub run_id: Option<i64>,
    pub quality_gate_id: Option<&'a str>,
    pub artifact_type: &'a str,
    pub file_name: &'a str,
    pub content_type: &'a str,
    pub summary: Option<&'a str>,
    pub bytes: &'a [u8],
    pub retention_days: i64,
}

fn response(row: DevRailArtifactRow) -> DevRailArtifactResponse {
    DevRailArtifactResponse {
        download_url: format!("/api/v1/artifacts/{}/download", row.id),
        id: row.id,
        project_id: row.project_id,
        task_id: row.task_id,
        run_id: row.run_id,
        quality_gate_id: row.quality_gate_id,
        artifact_type: row.artifact_type,
        file_name: row.file_name,
        content_type: row.content_type,
        byte_size: row.byte_size,
        sha256: row.sha256,
        summary: row.summary,
        cleanup_status: row.cleanup_status,
        cleanup_attempts: row.cleanup_attempts,
        expires_at: row.expires_at,
        deleted_at: row.deleted_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn valid_artifact_type(value: &str) -> bool {
    matches!(
        value,
        "test_report" | "patch" | "screenshot" | "video" | "trace" | "diagnosis" | "log" | "other"
    )
}

fn safe_file_name(value: &str) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 128 || trimmed.contains(['/', '\\']) {
        return Err(ApiError::validation("产物文件名无效"));
    }
    let normalized = trimmed
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.is_empty() || normalized == "." || normalized == ".." {
        return Err(ApiError::validation("产物文件名无效"));
    }
    Ok(normalized)
}

fn validate_storage_key(key: &str) -> Result<(), ApiError> {
    if key.is_empty() || key.len() > 512 || key.starts_with('/') {
        return Err(ApiError::validation("产物引用无效"));
    }
    if key
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ApiError::validation("产物引用无效"));
    }
    let mut components = Path::new(key).components();
    if components.any(|component| {
        matches!(component, Component::ParentDir | Component::CurDir)
            || !matches!(component, Component::Normal(_))
    }) {
        return Err(ApiError::validation("产物引用无效"));
    }
    Ok(())
}

async fn controlled_artifact_path(
    controlled_root: &Path,
    storage_key: &str,
) -> Result<PathBuf, ApiError> {
    validate_storage_key(storage_key)?;
    let root = fs::canonicalize(controlled_root)
        .await
        .map_err(ApiError::internal)?;
    let candidate = root.join(storage_key);
    let mut existing = candidate
        .parent()
        .ok_or_else(|| ApiError::validation("产物引用无效"))?
        .to_path_buf();
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| ApiError::validation("产物引用无效"))?
            .to_path_buf();
    }
    let canonical_existing = fs::canonicalize(existing)
        .await
        .map_err(ApiError::internal)?;
    if !canonical_existing.starts_with(&root) {
        return Err(ApiError::validation("产物不在受控根目录内"));
    }
    if candidate.exists() {
        let canonical_candidate = fs::canonicalize(&candidate)
            .await
            .map_err(ApiError::internal)?;
        if !canonical_candidate.starts_with(&root) {
            return Err(ApiError::validation("产物不在受控根目录内"));
        }
    }
    Ok(candidate)
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    query: &DevRailArtifactQuery,
) -> Result<DevRailArtifactPage, ApiError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    if let Some(kind) = query.artifact_type.as_deref() {
        if !valid_artifact_type(kind) {
            return Err(ApiError::validation("产物类型无效"));
        }
    }
    let rows = devrail_artifacts::list(
        pool,
        actor,
        query.task_id,
        query.run_id,
        query.artifact_type.as_deref(),
        page,
        page_size,
    )
    .await
    .map_err(ApiError::internal)?;
    let total = devrail_artifacts::count(
        pool,
        actor,
        query.task_id,
        query.run_id,
        query.artifact_type.as_deref(),
    )
    .await
    .map_err(ApiError::internal)?;
    Ok(DevRailArtifactPage {
        items: rows.into_iter().map(response).collect(),
        total,
        page,
        page_size,
    })
}

pub async fn get(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailArtifactResponse, ApiError> {
    let row = devrail_artifacts::find_by_id(pool, actor, id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("产物不存在"))?;
    Ok(response(row))
}

pub async fn download(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    controlled_root: &Path,
) -> Result<ArtifactDownload, ApiError> {
    let row = devrail_artifacts::find_by_id(pool, actor, id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("产物不存在"))?;
    let path = controlled_artifact_path(controlled_root, &row.storage_key).await?;
    let bytes = fs::read(&path)
        .await
        .map_err(|_| ApiError::not_found("产物内容不存在"))?;
    if bytes.len() > MAX_ARTIFACT_BYTES || bytes.len() as i64 != row.byte_size {
        return Err(ApiError::conflict("产物内容校验失败"));
    }
    let digest = hex::encode(Sha256::digest(&bytes));
    if digest != row.sha256 {
        return Err(ApiError::conflict("产物内容校验失败"));
    }
    Ok(ArtifactDownload {
        file_name: row.file_name,
        content_type: row.content_type,
        bytes,
    })
}

pub async fn store(
    pool: &PgPool,
    actor: &ActorContext,
    controlled_root: &Path,
    input: &StoreArtifactInput<'_>,
) -> Result<DevRailArtifactResponse, ApiError> {
    if input.project_id <= 0 || input.task_id <= 0 || input.retention_days <= 0 {
        return Err(ApiError::validation("产物归属或保留期无效"));
    }
    if !valid_artifact_type(input.artifact_type) {
        return Err(ApiError::validation("产物类型无效"));
    }
    if input.bytes.is_empty() || input.bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(ApiError::validation("产物大小必须为 1-16MB"));
    }
    let file_name = safe_file_name(input.file_name)?;
    let storage_key = format!("artifacts/{}/{}.bin", actor.organization_id, Uuid::new_v4());
    let path = controlled_artifact_path(controlled_root, &storage_key).await?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(ApiError::internal)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .map_err(ApiError::internal)?;
    if let Err(error) = file.write_all(input.bytes).await {
        let _ = fs::remove_file(&path).await;
        return Err(ApiError::internal(error));
    }
    file.flush().await.map_err(ApiError::internal)?;
    let digest = hex::encode(Sha256::digest(input.bytes));
    let expires_at = Utc::now() + Duration::days(input.retention_days);
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let row = match devrail_artifacts::insert(
        &mut transaction,
        &devrail_artifacts::NewArtifact {
            organization_id: actor.organization_id,
            department_id: actor.department_id,
            owner_user_id: actor.user_id,
            project_id: input.project_id,
            task_id: input.task_id,
            run_id: input.run_id,
            quality_gate_id: input.quality_gate_id,
            artifact_type: input.artifact_type,
            storage_key: &storage_key,
            file_name: &file_name,
            content_type: input.content_type,
            byte_size: input.bytes.len() as i64,
            sha256: &digest,
            summary: input.summary,
            expires_at,
        },
    )
    .await
    {
        Ok(row) => row,
        Err(error) => {
            let _ = transaction.rollback().await;
            let _ = fs::remove_file(&path).await;
            return Err(ApiError::internal(error));
        }
    };
    transaction.commit().await.map_err(|error| {
        let _ = std::fs::remove_file(&path);
        ApiError::internal(error)
    })?;
    Ok(response(row))
}

pub async fn cleanup_expired(
    pool: &PgPool,
    controlled_root: &Path,
    limit: i64,
) -> Result<u64, ApiError> {
    let claimed = devrail_artifacts::claim_expired(pool, limit)
        .await
        .map_err(ApiError::internal)?;
    let mut cleaned = 0_u64;
    for artifact in claimed {
        let result = match controlled_artifact_path(controlled_root, &artifact.storage_key).await {
            Ok(path) => match fs::remove_file(path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.to_string()),
            },
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = result {
            let _ = devrail_artifacts::mark_cleanup_failed(pool, artifact.id, &error).await;
            continue;
        }
        if devrail_artifacts::mark_deleted(pool, artifact.id)
            .await
            .map_err(ApiError::internal)?
        {
            cleaned += 1;
        }
    }
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_file_names_are_normalized_without_paths() {
        assert_eq!(safe_file_name("报告 1.txt").expect("safe name"), "___1.txt");
        assert!(safe_file_name("../secret").is_err());
        assert!(safe_file_name("/tmp/file").is_err());
    }

    #[test]
    fn storage_keys_reject_escape_components() {
        assert!(validate_storage_key("artifacts/1/file.bin").is_ok());
        assert!(validate_storage_key("../outside").is_err());
        assert!(validate_storage_key("/outside").is_err());
        assert!(validate_storage_key("artifacts/./file").is_err());
        assert!(validate_storage_key("artifacts//file").is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn final_symlink_cannot_escape_controlled_root() {
        let root = std::env::temp_dir().join(format!("devrail-artifact-root-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("devrail-artifact-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("artifacts/1"))
            .await
            .expect("create artifact root");
        fs::write(&outside, b"outside")
            .await
            .expect("create outside file");
        std::os::unix::fs::symlink(&outside, root.join("artifacts/1/link.bin"))
            .expect("create escaping artifact symlink");
        let result = controlled_artifact_path(&root, "artifacts/1/link.bin").await;
        assert!(result.is_err());
        let _ = fs::remove_file(&outside).await;
        let _ = fs::remove_dir_all(&root).await;
    }
}
