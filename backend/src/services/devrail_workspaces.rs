//! Task/attempt workspace lifecycle and controlled filesystem operations.

use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::{
    DevRailTaskWorkspaceResponse, DevRailTaskWorkspaceRow, RebuildDevRailTaskWorkspaceRequest,
};
use crate::repositories::{self, devrail, devrail_workspaces};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration as StdDuration;

pub(crate) struct MaterializedWorkspace {
    pub path: PathBuf,
    pub base_commit: Option<String>,
}

fn db_error(error: sqlx::Error) -> ApiError {
    ApiError::internal(error)
}

fn response(row: DevRailTaskWorkspaceRow) -> DevRailTaskWorkspaceResponse {
    DevRailTaskWorkspaceResponse {
        id: row.id,
        task_id: row.task_id,
        run_id: row.run_id,
        attempt: row.attempt,
        relative_id: row.relative_path,
        base_commit: row.base_commit,
        branch_name: row.branch_name,
        workflow_version: row.workflow_version,
        workflow_digest: row.workflow_digest,
        environment_version: row.environment_version,
        tool_versions: row.tool_versions,
        snapshot_digest: row.snapshot_digest,
        lifecycle_status: row.lifecycle_status,
        cleanup_status: row.cleanup_status,
        cleanup_attempts: row.cleanup_attempts,
        next_cleanup_at: row.next_cleanup_at,
        last_hook: row.last_hook,
        diagnostic_ref: row.diagnostic_ref,
        error_summary: row.error_summary,
        created_at: row.created_at,
        updated_at: row.updated_at,
        cleaned_at: row.cleaned_at,
    }
}

pub fn workspace_key(task_id: i64, attempt: i32) -> Result<String, ApiError> {
    if task_id <= 0 || attempt <= 0 {
        return Err(ApiError::validation("任务或执行尝试无效"));
    }
    let mut digest = Sha256::new();
    digest.update(format!("{task_id}:{attempt}"));
    let suffix = hex::encode(digest.finalize());
    Ok(format!(
        "task-{task_id}-attempt-{attempt}-{}",
        &suffix[..12]
    ))
}

pub fn path_digest(relative: &str) -> String {
    hex::encode(Sha256::digest(relative.as_bytes()))
}

fn hook_names(snapshot: &serde_json::Value, phase: &str) -> Result<Vec<String>, ApiError> {
    let key = match phase {
        "before_run" => "beforeRun",
        "after_run" => "afterRun",
        "on_failure" | "cleanup" => "afterRun",
        _ => return Err(ApiError::validation("未知工作区 hook")),
    };
    let hooks = snapshot
        .pointer("/config/hooks")
        .or_else(|| snapshot.get("hooks"))
        .and_then(|value| value.get(key).or_else(|| value.get(phase)));
    let Some(values) = hooks.and_then(serde_json::Value::as_array) else {
        return Ok(Vec::new());
    };
    let names = values
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if names
        .iter()
        .any(|name| !matches!(name.as_str(), "cargo-flow-scope" | "cargo-flow-verify"))
    {
        return Err(ApiError::validation("工作区 hook 不在平台白名单内"));
    }
    Ok(names)
}

/// Run the platform-owned hook commands. No shell is invoked and the process
/// inherits only the controlled workspace and a minimal PATH/HOME pair.
pub async fn run_hooks(
    snapshot: &serde_json::Value,
    phase: &str,
    cwd: &Path,
) -> Result<(), ApiError> {
    let names = hook_names(snapshot, phase)?;
    for name in names {
        // Legacy environments may expose only an app-server binary. They do
        // not carry the project gate configuration, so there is no hook to run.
        if !cwd.join(".arc-flow").exists() {
            continue;
        }
        let (program, args): (&str, &[&str]) = match name.as_str() {
            "cargo-flow-scope" => ("cargo", &["flow", "scope"]),
            "cargo-flow-verify" => ("cargo", &["flow", "verify", "--components", "backend"]),
            _ => unreachable!("hook_names validates the allowlist"),
        };
        let mut command = tokio::process::Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Ok(path) = std::env::var("PATH") {
            command.env("PATH", path);
        }
        if let Ok(home) = std::env::var("HOME") {
            command.env("HOME", home);
        }
        let output = tokio::time::timeout(StdDuration::from_secs(300), command.status())
            .await
            .map_err(|_| ApiError::conflict(format!("工作区 hook {name} 执行超时")))?
            .map_err(ApiError::internal)?;
        if !output.success() {
            return Err(ApiError::conflict(format!("工作区 hook {name} 执行失败")));
        }
    }
    Ok(())
}

pub async fn controlled_path(root: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| ApiError::validation("受控工作区根目录不存在或不可访问"))?;
    if Path::new(relative).components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return Err(ApiError::validation("工作区路径越界"));
    }
    let candidate = root.join(relative);
    if let Ok(metadata) = tokio::fs::symlink_metadata(&candidate).await {
        if metadata.file_type().is_symlink() {
            return Err(ApiError::validation("工作区路径不允许使用符号链接"));
        }
        let canonical = tokio::fs::canonicalize(&candidate)
            .await
            .map_err(ApiError::internal)?;
        if !canonical.starts_with(&root) {
            return Err(ApiError::validation("工作区路径不在受控根目录内"));
        }
    }
    Ok(candidate)
}

pub async fn materialize(root: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    tokio::fs::create_dir_all(root)
        .await
        .map_err(ApiError::internal)?;
    let candidate = controlled_path(root, relative).await?;
    tokio::fs::create_dir_all(&candidate)
        .await
        .map_err(ApiError::internal)?;
    let canonical = tokio::fs::canonicalize(&candidate)
        .await
        .map_err(ApiError::internal)?;
    let canonical_root = tokio::fs::canonicalize(root)
        .await
        .map_err(ApiError::internal)?;
    if !canonical.starts_with(canonical_root) {
        return Err(ApiError::validation("工作区路径不在受控根目录内"));
    }
    Ok(canonical)
}

fn copy_directory(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "source contains a symbolic link",
            ));
        }
        let destination = target.join(entry.file_name());
        if metadata.is_dir() {
            copy_directory(&entry.path(), &destination)?;
        } else if metadata.is_file() {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

async fn git_output(source: &Path, args: &[&str]) -> Result<String, ApiError> {
    let output = tokio::time::timeout(
        StdDuration::from_secs(60),
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(source)
            .args(args)
            .env_clear()
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| ApiError::conflict("Git 工作区操作超时"))?
    .map_err(ApiError::internal)?;
    if !output.status.success() {
        return Err(ApiError::conflict("无法创建 Git worktree"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) async fn materialize_from_source(
    root: &Path,
    source: &Path,
    relative: &str,
    branch: Option<&str>,
) -> Result<MaterializedWorkspace, ApiError> {
    tokio::fs::create_dir_all(root)
        .await
        .map_err(ApiError::internal)?;
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(ApiError::internal)?;
    let source = tokio::fs::canonicalize(source)
        .await
        .map_err(|_| ApiError::validation("环境工作区不存在或不可访问"))?;
    if !source.starts_with(&root) || source == root {
        return Err(ApiError::validation("环境工作区不在受控根目录内"));
    }
    let target = controlled_path(&root, relative).await?;
    if tokio::fs::try_exists(&target)
        .await
        .map_err(ApiError::internal)?
    {
        let base_commit = if target.join(".git").exists() {
            git_output(&target, &["rev-parse", "HEAD"]).await.ok()
        } else {
            None
        };
        return Ok(MaterializedWorkspace {
            path: tokio::fs::canonicalize(target)
                .await
                .map_err(ApiError::internal)?,
            base_commit,
        });
    }
    let reference = branch.unwrap_or("HEAD");
    if source.join(".git").exists() {
        let commit = git_output(
            &source,
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        )
        .await?;
        let target_string = target.to_string_lossy().into_owned();
        git_output(
            &source,
            &["worktree", "add", "--detach", &target_string, &commit],
        )
        .await?;
        return Ok(MaterializedWorkspace {
            path: tokio::fs::canonicalize(target)
                .await
                .map_err(ApiError::internal)?,
            base_commit: Some(commit),
        });
    }
    let source_copy = source.clone();
    let target_copy = target.clone();
    tokio::task::spawn_blocking(move || copy_directory(&source_copy, &target_copy))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::internal)?;
    Ok(MaterializedWorkspace {
        path: tokio::fs::canonicalize(target)
            .await
            .map_err(ApiError::internal)?,
        base_commit: None,
    })
}

pub async fn get_for_task(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
) -> Result<Option<DevRailTaskWorkspaceResponse>, ApiError> {
    devrail_workspaces::find_latest_for_task(pool, actor, task_id)
        .await
        .map_err(db_error)
        .map(|row| row.map(response))
}

pub async fn get_for_run(
    pool: &PgPool,
    actor: &ActorContext,
    run_id: i64,
) -> Result<DevRailTaskWorkspaceResponse, ApiError> {
    devrail_workspaces::find_by_run(pool, actor, run_id)
        .await
        .map_err(db_error)?
        .map(response)
        .ok_or_else(|| ApiError::not_found("运行工作区不存在或超出数据范围"))
}

pub async fn rebuild_for_task(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    root: &Path,
    request: &RebuildDevRailTaskWorkspaceRequest,
) -> Result<DevRailTaskWorkspaceResponse, ApiError> {
    let task = devrail::find_task_by_id(pool, actor, task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    let existing = devrail_workspaces::find_latest_for_task(pool, actor, task_id)
        .await
        .map_err(db_error)?;
    let attempt = existing.as_ref().map_or(1, |row| row.attempt);
    let key = workspace_key(task_id, attempt)?;
    let relative = key.clone();
    let digest = hex::encode(Sha256::digest(relative.as_bytes()));
    let row = if let Some(row) = existing {
        row
    } else {
        let mut tx = pool.begin().await.map_err(db_error)?;
        let row = devrail_workspaces::create(
            &mut tx,
            &devrail_workspaces::NewWorkspace {
                actor,
                task_id,
                run_id: None,
                attempt,
                workspace_key: &key,
                relative_path: &relative,
                path_digest: &digest,
                repository_id: task.repository_id,
                environment_id: task.environment_id,
                base_commit: None,
                branch_name: None,
                workflow_version: Some(&task.workflow_version),
                workflow_digest: Some(&task.workflow_digest),
                environment_version: None,
                tool_versions: &serde_json::json!({}),
                snapshot_digest: request
                    .snapshot_digest
                    .as_deref()
                    .or(Some(&task.dispatch_snapshot_digest)),
            },
        )
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::conflict("工作区已被其他请求占用"))?;
        tx.commit().await.map_err(db_error)?;
        row
    };
    materialize(root, &row.relative_path).await?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    devrail_workspaces::set_lifecycle(&mut tx, row.id, "ready", "pending", Some("rebuild"), None)
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record_actor(
        &mut tx,
        actor,
        "devrail.workspace.rebuild",
        "devrail_task_workspace",
        Some(row.id),
        serde_json::json!({"taskId": task_id, "attempt": row.attempt}),
    )
    .await
    .map_err(db_error)?;
    repositories::devrail_notifications::outbox(
        &mut tx,
        actor.organization_id,
        "workspace.rebuilt",
        "devrail_task_workspace",
        Some(row.id),
        &serde_json::json!({"workspaceId": row.id, "taskId": task_id, "status": "ready"}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    crate::app_metrics::record_workspace_event("rebuild", "succeeded");
    get_for_task(pool, actor, task_id)
        .await?
        .ok_or_else(|| ApiError::internal("工作区创建后无法读取"))
}

pub async fn cleanup(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    root: &Path,
) -> Result<DevRailTaskWorkspaceResponse, ApiError> {
    let row = devrail_workspaces::find_by_id(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("工作区不存在或超出数据范围"))?;
    if row.lifecycle_status == "cleaned" {
        return Ok(response(row));
    }
    let candidate = controlled_path(root, &row.relative_path).await?;
    let result = tokio::fs::remove_dir_all(&candidate).await;
    let mut tx = pool.begin().await.map_err(db_error)?;
    match result {
        Ok(()) => {
            devrail_workspaces::set_lifecycle(
                &mut tx,
                id,
                "cleaned",
                "completed",
                Some("cleanup"),
                None,
            )
            .await
            .map_err(db_error)?;
            repositories::audit_logs::record_actor(
                &mut tx,
                actor,
                "devrail.workspace.cleanup",
                "devrail_task_workspace",
                Some(id),
                serde_json::json!({"status": "cleaned"}),
            )
            .await
            .map_err(db_error)?;
            repositories::devrail_notifications::outbox(
                &mut tx,
                actor.organization_id,
                "workspace.cleaned",
                "devrail_task_workspace",
                Some(id),
                &serde_json::json!({"workspaceId": id, "status": "cleaned"}),
            )
            .await
            .map_err(db_error)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            devrail_workspaces::set_lifecycle(
                &mut tx,
                id,
                "cleaned",
                "completed",
                Some("cleanup"),
                None,
            )
            .await
            .map_err(db_error)?;
            repositories::audit_logs::record_actor(
                &mut tx,
                actor,
                "devrail.workspace.cleanup",
                "devrail_task_workspace",
                Some(id),
                serde_json::json!({"status": "cleaned"}),
            )
            .await
            .map_err(db_error)?;
            repositories::devrail_notifications::outbox(
                &mut tx,
                actor.organization_id,
                "workspace.cleaned",
                "devrail_task_workspace",
                Some(id),
                &serde_json::json!({"workspaceId": id, "status": "cleaned"}),
            )
            .await
            .map_err(db_error)?;
        }
        Err(error) => {
            let next = Utc::now() + Duration::seconds(30);
            devrail_workspaces::mark_cleanup_retry(&mut tx, id, next, "工作区清理暂时失败")
                .await
                .map_err(db_error)?;
            tx.commit().await.map_err(db_error)?;
            crate::app_metrics::record_workspace_event("cleanup", "retry");
            return Err(ApiError::conflict(format!(
                "工作区清理失败: {}",
                error.kind()
            )));
        }
    }
    tx.commit().await.map_err(db_error)?;
    crate::app_metrics::record_workspace_event("cleanup", "succeeded");
    devrail_workspaces::find_by_id(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(response)
        .ok_or_else(|| ApiError::not_found("工作区不存在"))
}

/// Worker-only reconciliation for cleanup records. It is intentionally
/// idempotent and never removes the database evidence of a workspace.
pub async fn reconcile_cleanup(pool: &PgPool, root: &Path) -> Result<usize, sqlx::Error> {
    let candidates = devrail_workspaces::list_cleanup_candidates(pool, 100).await?;
    let mut cleaned = 0;
    for row in candidates {
        let path = match controlled_path(root, &row.relative_path).await {
            Ok(path) => path,
            Err(_) => {
                let mut tx = pool.begin().await?;
                let next = Utc::now() + Duration::seconds(60);
                devrail_workspaces::mark_cleanup_retry(&mut tx, row.id, next, "受控路径校验失败")
                    .await?;
                tx.commit().await?;
                crate::app_metrics::record_workspace_event("reconcile", "retry");
                continue;
            }
        };
        let result = tokio::fs::remove_dir_all(path).await;
        let mut tx = pool.begin().await?;
        match result {
            Ok(()) => {
                devrail_workspaces::set_lifecycle(
                    &mut tx,
                    row.id,
                    "cleaned",
                    "completed",
                    Some("cleanup"),
                    None,
                )
                .await?;
                cleaned += 1;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                devrail_workspaces::set_lifecycle(
                    &mut tx,
                    row.id,
                    "cleaned",
                    "completed",
                    Some("cleanup"),
                    None,
                )
                .await?;
                cleaned += 1;
            }
            Err(_) => {
                let next = Utc::now() + Duration::seconds(60);
                devrail_workspaces::mark_cleanup_retry(&mut tx, row.id, next, "工作区清理暂时失败")
                    .await?;
            }
        }
        tx.commit().await?;
        crate::app_metrics::record_workspace_event("reconcile", "succeeded");
    }
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_keys_are_deterministic_and_attempt_scoped() {
        let first = workspace_key(42, 1).expect("key");
        assert_eq!(first, workspace_key(42, 1).expect("same key"));
        assert_ne!(first, workspace_key(42, 2).expect("different attempt"));
        assert!(first.starts_with("task-42-attempt-1-"));
    }

    #[test]
    fn workspace_keys_reject_non_positive_ids() {
        assert!(workspace_key(0, 1).is_err());
        assert!(workspace_key(1, 0).is_err());
    }

    #[test]
    fn hooks_are_closed_and_unknown_names_are_rejected() {
        let snapshot = serde_json::json!({"config":{"hooks":{"beforeRun":["cargo-flow-scope"]}}});
        assert_eq!(
            hook_names(&snapshot, "before_run").expect("hooks"),
            vec!["cargo-flow-scope"]
        );
        let invalid = serde_json::json!({"config":{"hooks":{"beforeRun":["rm -rf"]}}});
        assert!(hook_names(&invalid, "before_run").is_err());
    }

    #[tokio::test]
    async fn controlled_paths_reject_escape_and_materialize_inside_root() {
        let root =
            PathBuf::from("/tmp").join(format!("devrail-workspace-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.expect("root");
        assert!(controlled_path(&root, "../outside").await.is_err());
        assert!(controlled_path(&root, "/outside").await.is_err());
        let path = materialize(&root, "task-1-attempt-1")
            .await
            .expect("materialize");
        assert!(path.starts_with(
            tokio::fs::canonicalize(&root)
                .await
                .expect("canonical root")
        ));
        tokio::fs::remove_dir_all(&root).await.expect("cleanup");
    }

    #[tokio::test]
    async fn postgres_workspace_round_trip_is_scoped_and_idempotent() {
        let _guard = crate::db::DATABASE_TEST_LOCK.lock().await;
        let Some(database_url) = std::env::var("TEST_DATABASE_URL").ok() else {
            return;
        };
        let pool = crate::db::init_pool(&database_url)
            .await
            .expect("connect test database");
        crate::db::run_migrations(&pool)
            .await
            .expect("workspace migration");
        let root =
            PathBuf::from("/tmp").join(format!("devrail-workspace-db-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.expect("root");
        let fixture = crate::repositories::devrail_runs::create_workflow_e2e_fixture(
            &pool,
            root.to_string_lossy().as_ref(),
        )
        .await
        .expect("fixture");
        let key = workspace_key(fixture.task_id, 1).expect("key");
        let digest = path_digest(&key);
        let mut tx = pool.begin().await.expect("transaction");
        let created = devrail_workspaces::create(
            &mut tx,
            &devrail_workspaces::NewWorkspace {
                actor: &fixture.actor,
                task_id: fixture.task_id,
                run_id: None,
                attempt: 1,
                workspace_key: &key,
                relative_path: &key,
                path_digest: &digest,
                repository_id: None,
                environment_id: Some(fixture.environment_id),
                base_commit: None,
                branch_name: None,
                workflow_version: Some("devrail-v1"),
                workflow_digest: Some(&"a".repeat(64)),
                environment_version: None,
                tool_versions: &serde_json::json!({}),
                snapshot_digest: None,
            },
        )
        .await
        .expect("insert")
        .expect("created");
        tx.commit().await.expect("commit");
        let loaded = devrail_workspaces::find_by_id(&pool, &fixture.actor, created.id)
            .await
            .expect("load")
            .expect("workspace");
        assert_eq!(loaded.workspace_key, key);
        assert_eq!(loaded.path_digest, digest);
        let mut duplicate_tx = pool.begin().await.expect("transaction");
        let duplicate = devrail_workspaces::create(
            &mut duplicate_tx,
            &devrail_workspaces::NewWorkspace {
                actor: &fixture.actor,
                task_id: fixture.task_id,
                run_id: None,
                attempt: 1,
                workspace_key: &key,
                relative_path: &key,
                path_digest: &digest,
                repository_id: None,
                environment_id: Some(fixture.environment_id),
                base_commit: None,
                branch_name: None,
                workflow_version: Some("devrail-v1"),
                workflow_digest: Some(&"a".repeat(64)),
                environment_version: None,
                tool_versions: &serde_json::json!({}),
                snapshot_digest: None,
            },
        )
        .await
        .expect("duplicate insert");
        assert!(duplicate.is_none());
        duplicate_tx.rollback().await.expect("rollback");
        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }
}
