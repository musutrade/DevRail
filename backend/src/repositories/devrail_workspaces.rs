//! Scoped persistence for task/attempt workspaces.

use crate::access::ActorContext;
use crate::models::DevRailTaskWorkspaceRow;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const WORKSPACE_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, task_id, run_id, attempt, workspace_key, relative_path, path_digest, repository_id, environment_id, base_commit, branch_name, workflow_version, workflow_digest, environment_version, tool_versions, snapshot_digest, lifecycle_status, cleanup_status, cleanup_attempts, next_cleanup_at, last_hook, diagnostic_ref, error_summary, created_at, updated_at, cleaned_at";

fn scope(alias: &str) -> String {
    format!(
        "{alias}.organization_id = $2 AND ($1 = 'all' OR $1 = 'organization' OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments)))"
    )
}

pub(crate) struct NewWorkspace<'a> {
    pub actor: &'a ActorContext,
    pub task_id: i64,
    pub run_id: Option<i64>,
    pub attempt: i32,
    pub workspace_key: &'a str,
    pub relative_path: &'a str,
    pub path_digest: &'a str,
    pub repository_id: Option<i64>,
    pub environment_id: Option<i64>,
    pub base_commit: Option<&'a str>,
    pub branch_name: Option<&'a str>,
    pub workflow_version: Option<&'a str>,
    pub workflow_digest: Option<&'a str>,
    pub environment_version: Option<&'a str>,
    pub tool_versions: &'a serde_json::Value,
    pub snapshot_digest: Option<&'a str>,
}

pub async fn find_by_id(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailTaskWorkspaceRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskWorkspaceRow>(AssertSqlSafe(format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {WORKSPACE_COLUMNS} FROM devrail_task_workspaces w WHERE w.id=$5 AND {}",
        scope("w")
    )))
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(actor.department_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_run(
    pool: &PgPool,
    actor: &ActorContext,
    run_id: i64,
) -> Result<Option<DevRailTaskWorkspaceRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskWorkspaceRow>(AssertSqlSafe(format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {WORKSPACE_COLUMNS} FROM devrail_task_workspaces w WHERE w.run_id=$5 AND {}",
        scope("w")
    )))
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(actor.department_id)
    .bind(run_id)
    .fetch_optional(pool)
    .await
}

pub async fn find_latest_for_task(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
) -> Result<Option<DevRailTaskWorkspaceRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskWorkspaceRow>(AssertSqlSafe(format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {WORKSPACE_COLUMNS} FROM devrail_task_workspaces w WHERE w.task_id=$5 AND {} ORDER BY w.attempt DESC LIMIT 1",
        scope("w")
    )))
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(actor.department_id)
    .bind(task_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn create(
    connection: &mut PgConnection,
    input: &NewWorkspace<'_>,
) -> Result<Option<DevRailTaskWorkspaceRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskWorkspaceRow>(AssertSqlSafe(format!(
        "INSERT INTO devrail_task_workspaces (organization_id, department_id, owner_user_id, task_id, run_id, attempt, workspace_key, relative_path, path_digest, repository_id, environment_id, base_commit, branch_name, workflow_version, workflow_digest, environment_version, tool_versions, snapshot_digest) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18) ON CONFLICT (organization_id, workspace_key) DO NOTHING RETURNING {WORKSPACE_COLUMNS}"
    )))
    .bind(input.actor.organization_id)
    .bind(input.actor.department_id)
    .bind(input.actor.user_id)
    .bind(input.task_id)
    .bind(input.run_id)
    .bind(input.attempt)
    .bind(input.workspace_key)
    .bind(input.relative_path)
    .bind(input.path_digest)
    .bind(input.repository_id)
    .bind(input.environment_id)
    .bind(input.base_commit)
    .bind(input.branch_name)
    .bind(input.workflow_version)
    .bind(input.workflow_digest)
    .bind(input.environment_version)
    .bind(input.tool_versions)
    .bind(input.snapshot_digest)
    .fetch_optional(connection)
    .await
}

pub async fn set_lifecycle(
    connection: &mut PgConnection,
    id: i64,
    lifecycle_status: &str,
    cleanup_status: &str,
    last_hook: Option<&str>,
    error_summary: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_task_workspaces SET lifecycle_status=$2, cleanup_status=$3, last_hook=$4, error_summary=$5, cleaned_at=CASE WHEN $2='cleaned' THEN COALESCE(cleaned_at,now()) ELSE cleaned_at END, updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(lifecycle_status)
    .bind(cleanup_status)
    .bind(last_hook)
    .bind(error_summary)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub(crate) async fn set_base_commit(
    connection: &mut PgConnection,
    id: i64,
    base_commit: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_task_workspaces SET base_commit=COALESCE($2,base_commit), updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(base_commit)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn mark_cleanup_retry(
    connection: &mut PgConnection,
    id: i64,
    next_cleanup_at: chrono::DateTime<chrono::Utc>,
    error_summary: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_task_workspaces SET lifecycle_status='cleanup_failed', cleanup_status='failed', cleanup_attempts=cleanup_attempts+1, next_cleanup_at=$2, error_summary=$3, updated_at=now() WHERE id=$1 AND lifecycle_status <> 'cleaned'",
    )
    .bind(id)
    .bind(next_cleanup_at)
    .bind(error_summary)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub(crate) async fn mark_cleanup_pending_for_run(
    connection: &mut PgConnection,
    organization_id: i64,
    run_id: i64,
    hook: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_task_workspaces SET lifecycle_status='cleanup_pending', cleanup_status='pending', last_hook=$3, updated_at=now() WHERE organization_id=$1 AND run_id=$2 AND lifecycle_status <> 'cleaned'",
    )
    .bind(organization_id)
    .bind(run_id)
    .bind(hook)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_cleanup_candidates(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<DevRailTaskWorkspaceRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskWorkspaceRow>(AssertSqlSafe(format!(
        "SELECT {WORKSPACE_COLUMNS} FROM devrail_task_workspaces WHERE lifecycle_status IN ('cleanup_pending','cleanup_failed','orphaned') AND (next_cleanup_at IS NULL OR next_cleanup_at <= now()) ORDER BY updated_at LIMIT $1"
    )))
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await
}
