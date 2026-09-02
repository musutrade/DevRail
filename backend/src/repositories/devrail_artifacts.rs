//! Scoped persistence for controlled run artifacts.

use crate::access::ActorContext;
use crate::models::DevRailArtifactRow;
use sqlx::{AssertSqlSafe, PgConnection, PgPool, Row};

const ARTIFACT_COLUMNS: &str = "a.id, a.organization_id, a.department_id, a.owner_user_id, a.project_id, a.task_id, a.run_id, a.quality_gate_id, a.artifact_type, a.storage_key, a.file_name, a.content_type, a.byte_size, a.sha256, a.summary, a.cleanup_status, a.cleanup_attempts, a.next_cleanup_at, a.last_cleanup_error, a.expires_at, a.deleted_at, a.created_at, a.updated_at";

fn scope(alias: &str) -> String {
    format!(
        "{alias}.organization_id = $2 AND ($1 = 'all' OR $1 = 'organization' OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments)))"
    )
}

pub(crate) struct NewArtifact<'a> {
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub task_id: i64,
    pub run_id: Option<i64>,
    pub quality_gate_id: Option<&'a str>,
    pub artifact_type: &'a str,
    pub storage_key: &'a str,
    pub file_name: &'a str,
    pub content_type: &'a str,
    pub byte_size: i64,
    pub sha256: &'a str,
    pub summary: Option<&'a str>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ArtifactCleanupRow {
    pub id: i64,
    pub storage_key: String,
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: Option<i64>,
    run_id: Option<i64>,
    artifact_type: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Vec<DevRailArtifactRow>, sqlx::Error> {
    let sql = format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {ARTIFACT_COLUMNS} FROM devrail_artifacts a WHERE a.deleted_at IS NULL AND ($5::bigint IS NULL OR a.task_id=$5) AND ($6::bigint IS NULL OR a.run_id=$6) AND ($7::text IS NULL OR a.artifact_type=$7) AND {} ORDER BY a.created_at DESC,a.id DESC LIMIT $8 OFFSET $9",
        scope("a")
    );
    sqlx::query_as::<_, DevRailArtifactRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
        .bind(run_id)
        .bind(artifact_type)
        .bind(page_size)
        .bind((page - 1) * page_size)
        .fetch_all(pool)
        .await
}

pub async fn count(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: Option<i64>,
    run_id: Option<i64>,
    artifact_type: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let sql = format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT count(*) FROM devrail_artifacts a WHERE a.deleted_at IS NULL AND ($5::bigint IS NULL OR a.task_id=$5) AND ($6::bigint IS NULL OR a.run_id=$6) AND ($7::text IS NULL OR a.artifact_type=$7) AND {}",
        scope("a")
    );
    sqlx::query(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
        .bind(run_id)
        .bind(artifact_type)
        .fetch_one(pool)
        .await?
        .try_get(0)
}

pub async fn find_by_id(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailArtifactRow>, sqlx::Error> {
    let sql = format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {ARTIFACT_COLUMNS} FROM devrail_artifacts a WHERE a.id=$5 AND a.deleted_at IS NULL AND {}",
        scope("a")
    );
    sqlx::query_as::<_, DevRailArtifactRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn insert(
    connection: &mut PgConnection,
    input: &NewArtifact<'_>,
) -> Result<DevRailArtifactRow, sqlx::Error> {
    sqlx::query_as::<_, DevRailArtifactRow>(AssertSqlSafe(format!(
        "INSERT INTO devrail_artifacts AS a (organization_id,department_id,owner_user_id,project_id,task_id,run_id,quality_gate_id,artifact_type,storage_key,file_name,content_type,byte_size,sha256,summary,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING {ARTIFACT_COLUMNS}"
    )))
    .bind(input.organization_id)
    .bind(input.department_id)
    .bind(input.owner_user_id)
    .bind(input.project_id)
    .bind(input.task_id)
    .bind(input.run_id)
    .bind(input.quality_gate_id)
    .bind(input.artifact_type)
    .bind(input.storage_key)
    .bind(input.file_name)
    .bind(input.content_type)
    .bind(input.byte_size)
    .bind(input.sha256)
    .bind(input.summary)
    .bind(input.expires_at)
    .fetch_one(connection)
    .await
}

pub(crate) async fn claim_expired(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<ArtifactCleanupRow>, sqlx::Error> {
    sqlx::query_as::<_, ArtifactCleanupRow>(
        "WITH candidates AS (SELECT id FROM devrail_artifacts WHERE deleted_at IS NULL AND cleanup_status IN ('pending','failed') AND expires_at <= now() AND (next_cleanup_at IS NULL OR next_cleanup_at <= now()) ORDER BY expires_at,id LIMIT $1 FOR UPDATE SKIP LOCKED) UPDATE devrail_artifacts a SET cleanup_status='running',cleanup_attempts=a.cleanup_attempts+1,updated_at=now() FROM candidates WHERE a.id=candidates.id RETURNING a.id,a.storage_key",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub(crate) async fn mark_deleted(pool: &PgPool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_artifacts SET cleanup_status='deleted',deleted_at=COALESCE(deleted_at,now()),last_cleanup_error=NULL,next_cleanup_at=NULL,updated_at=now() WHERE id=$1 AND cleanup_status='running'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn mark_cleanup_failed(
    pool: &PgPool,
    id: i64,
    error: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_artifacts SET cleanup_status='failed',last_cleanup_error=$2,next_cleanup_at=now()+make_interval(secs => LEAST(3600,POWER(2,cleanup_attempts)::int)),updated_at=now() WHERE id=$1 AND cleanup_status='running'",
    )
    .bind(id)
    .bind(error.chars().take(500).collect::<String>())
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn artifact_insert_returns_the_scoped_row() {
        let Ok(fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = fixture.pool().clone();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let (owner_user_id, organization_id, department_id, task_id) =
            crate::repositories::devrail_runs::create_harness_test_task(&pool, &suffix)
                .await
                .expect("create artifact test task");
        let project_id = sqlx::query_scalar::<_, i64>(
            "SELECT project_id FROM devrail_tasks WHERE organization_id=$1 AND id=$2",
        )
        .bind(organization_id)
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("read artifact test project");
        let storage_key = format!("artifacts/{organization_id}/{suffix}.bin");
        let digest = "0".repeat(64);
        let expires_at = chrono::Utc::now() + chrono::Duration::days(1);

        let mut transaction = pool.begin().await.expect("begin artifact insert");
        let artifact = insert(
            &mut transaction,
            &NewArtifact {
                organization_id,
                department_id,
                owner_user_id,
                project_id,
                task_id,
                run_id: None,
                quality_gate_id: Some("repository-insert-regression"),
                artifact_type: "log",
                storage_key: &storage_key,
                file_name: "gate.log",
                content_type: "text/plain",
                byte_size: 4,
                sha256: &digest,
                summary: Some("质量门禁日志"),
                expires_at,
            },
        )
        .await
        .expect("insert artifact and return scoped row");
        assert_eq!(artifact.organization_id, organization_id);
        assert_eq!(artifact.project_id, project_id);
        assert_eq!(artifact.task_id, task_id);
        assert_eq!(
            artifact.quality_gate_id.as_deref(),
            Some("repository-insert-regression")
        );
        transaction
            .rollback()
            .await
            .expect("rollback artifact insert");

        drop(pool);
        fixture.cleanup().await.expect("cleanup artifact schema");
    }
}
