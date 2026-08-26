//! DevRail Phase 0 data access.  Every read and scoped write is constrained by
//! the authenticated actor's organization and data scope in SQL.

use crate::access::ActorContext;
use crate::models::{
    DevRailEnvironmentRow, DevRailListQuery, DevRailProjectRow, DevRailRepositoryRow,
    DevRailTaskRow,
};
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const PROJECT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, slug, name, description, status, default_repository_id, default_environment_id, notification_policy, quality_gate_template, created_at, updated_at, archived_at";
const REPOSITORY_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, name, remote_url, protocol, default_branch, credential_ref, last_sync_status, last_head_sha, last_remote_branch, last_remote_branch_count, created_at, updated_at, archived_at";
const ENVIRONMENT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, name, workspace_root, network_mode, tool_policy, secret_refs, max_duration_secs, enabled, created_at, updated_at, archived_at";
const TASK_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, repository_id, environment_id, assignee_user_id, title, goal, background, acceptance_criteria, constraints, priority, status, revision, dispatch_snapshot, dispatch_snapshot_digest, workflow_source, workflow_version, workflow_digest, scheduler_attempt, scheduler_retry_count, scheduler_max_attempts, scheduler_retry_at, scheduler_last_error, labels, due_at, created_at, updated_at, archived_at";
const SCHEDULER_TASK_COLUMNS: &str = "t.id, t.organization_id, t.department_id, t.owner_user_id, t.project_id, t.repository_id, t.environment_id, t.assignee_user_id, t.title, t.goal, t.background, t.acceptance_criteria, t.constraints, t.priority, t.status, t.revision, t.dispatch_snapshot, t.dispatch_snapshot_digest, t.workflow_source, t.workflow_version, t.workflow_digest, t.scheduler_attempt, t.scheduler_retry_count, t.scheduler_max_attempts, t.scheduler_retry_at, t.scheduler_last_error, t.labels, t.due_at, t.created_at, t.updated_at, t.archived_at";

pub(crate) struct NewProject<'a> {
    pub slug: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub department_id: Option<i64>,
    pub notification_policy: &'a Value,
    pub quality_gate_template: &'a Value,
}
pub(crate) struct ProjectUpdate<'a> {
    pub name: Option<&'a str>,
    pub description_set: bool,
    pub description: Option<&'a str>,
    pub department_set: bool,
    pub department_id: Option<i64>,
    pub status: Option<&'a str>,
    pub default_repository_set: bool,
    pub default_repository_id: Option<i64>,
    pub default_environment_set: bool,
    pub default_environment_id: Option<i64>,
    pub notification_policy: Option<&'a Value>,
    pub quality_gate_template: Option<&'a Value>,
}
pub(crate) struct NewRepository<'a> {
    pub project_id: i64,
    pub name: &'a str,
    pub remote_url: &'a str,
    pub protocol: &'a str,
    pub default_branch: &'a str,
    pub credential_ref: Option<&'a str>,
    pub department_id: Option<i64>,
}
pub(crate) struct RepositoryUpdate<'a> {
    pub name: Option<&'a str>,
    pub remote_url: Option<&'a str>,
    pub protocol: Option<&'a str>,
    pub default_branch: Option<&'a str>,
    pub credential_set: bool,
    pub credential_ref: Option<&'a str>,
    pub status: Option<&'a str>,
}
pub(crate) struct RepositorySyncUpdate<'a> {
    pub project_id: i64,
    pub id: i64,
    pub status: &'a str,
    pub head_sha: Option<&'a str>,
    pub remote_branch: Option<&'a str>,
    pub remote_branch_count: Option<i64>,
}
pub(crate) struct NewEnvironment<'a> {
    pub project_id: i64,
    pub name: &'a str,
    pub workspace_root: &'a str,
    pub network_mode: &'a str,
    pub tool_policy: &'a Value,
    pub secret_refs: &'a Value,
    pub max_duration_secs: i64,
    pub enabled: bool,
    pub department_id: Option<i64>,
}
pub(crate) struct EnvironmentUpdate<'a> {
    pub name: Option<&'a str>,
    pub workspace_root: Option<&'a str>,
    pub network_mode: Option<&'a str>,
    pub tool_policy: Option<&'a Value>,
    pub secret_refs: Option<&'a Value>,
    pub max_duration_secs: Option<i64>,
    pub enabled: Option<bool>,
}
pub(crate) struct NewTask<'a> {
    pub project_id: i64,
    pub repository_id: Option<i64>,
    pub environment_id: Option<i64>,
    pub assignee_user_id: Option<i64>,
    pub title: &'a str,
    pub goal: &'a str,
    pub background: Option<&'a str>,
    pub acceptance_criteria: Option<&'a str>,
    pub constraints: Option<&'a str>,
    pub priority: &'a str,
    pub labels: &'a Value,
    pub due_at: Option<chrono::DateTime<chrono::Utc>>,
    pub department_id: Option<i64>,
}
pub(crate) struct TaskUpdate<'a> {
    pub title: Option<&'a str>,
    pub goal: Option<&'a str>,
    pub background_set: bool,
    pub background: Option<&'a str>,
    pub acceptance_set: bool,
    pub acceptance_criteria: Option<&'a str>,
    pub constraints_set: bool,
    pub constraints: Option<&'a str>,
    pub priority: Option<&'a str>,
    pub status: Option<&'a str>,
    pub assignee_set: bool,
    pub assignee_user_id: Option<i64>,
    pub labels: Option<&'a Value>,
    pub due_at_set: bool,
    pub due_at: Option<chrono::DateTime<chrono::Utc>>,
    pub repository_set: bool,
    pub repository_id: Option<i64>,
    pub environment_set: bool,
    pub environment_id: Option<i64>,
    pub queue_snapshot: Option<&'a Value>,
    pub queue_snapshot_digest: Option<&'a str>,
    pub workflow_source: Option<&'a str>,
    pub workflow_version: Option<&'a str>,
    pub workflow_digest: Option<&'a str>,
    pub queue_max_attempts: Option<i32>,
}

fn scope_sql(alias: &str) -> String {
    format!(
        "($1 = 'all' OR {alias}.organization_id = $2 AND ($1 = 'organization' OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments))))"
    )
}

async fn project_scope(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailProjectRow>, sqlx::Error> {
    let sql = format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id = $4 AND organization_id = $2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id = parent.id WHERE child.organization_id = $2) SELECT {PROJECT_COLUMNS} FROM devrail_projects p WHERE p.id = $5 AND p.archived_at IS NULL AND {}",
        scope_sql("p")
    );
    sqlx::query_as::<_, DevRailProjectRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn list_projects(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
    page: i64,
    size: i64,
) -> Result<Vec<DevRailProjectRow>, sqlx::Error> {
    let sql = format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id = $4 AND organization_id = $2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id = parent.id WHERE child.organization_id = $2) SELECT {PROJECT_COLUMNS} FROM devrail_projects p WHERE {} AND ($5::text IS NULL OR p.status = $5) AND ($6::text IS NULL OR p.slug ILIKE '%' || $6 || '%' OR p.name ILIKE '%' || $6 || '%') ORDER BY p.updated_at DESC, p.id DESC LIMIT $7 OFFSET $8",
        scope_sql("p")
    );
    sqlx::query_as::<_, DevRailProjectRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(q.status.as_deref())
        .bind(q.keyword.as_deref())
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(pool)
        .await
}

pub async fn count_projects(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<i64, sqlx::Error> {
    let sql = format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id = $4 AND organization_id = $2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id = parent.id WHERE child.organization_id = $2) SELECT count(*) FROM devrail_projects p WHERE {} AND ($5::text IS NULL OR p.status = $5) AND ($6::text IS NULL OR p.slug ILIKE '%' || $6 || '%' OR p.name ILIKE '%' || $6 || '%')",
        scope_sql("p")
    );
    sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(q.status.as_deref())
        .bind(q.keyword.as_deref())
        .fetch_one(pool)
        .await
}

pub async fn find_project(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailProjectRow>, sqlx::Error> {
    project_scope(pool, actor, id).await
}

pub(crate) async fn create_project(
    c: &mut PgConnection,
    actor: &ActorContext,
    n: &NewProject<'_>,
) -> Result<DevRailProjectRow, sqlx::Error> {
    let sql = format!("INSERT INTO devrail_projects (organization_id, department_id, owner_user_id, slug, name, description, notification_policy, quality_gate_template) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING {PROJECT_COLUMNS}");
    sqlx::query_as::<_, DevRailProjectRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(n.department_id)
        .bind(actor.user_id)
        .bind(n.slug)
        .bind(n.name)
        .bind(n.description)
        .bind(n.notification_policy)
        .bind(n.quality_gate_template)
        .fetch_one(c)
        .await
}

pub(crate) async fn update_project(
    c: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
    u: &ProjectUpdate<'_>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$17 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) UPDATE devrail_projects SET name = COALESCE($3,name), description = CASE WHEN $4 THEN $5 ELSE description END, department_id = CASE WHEN $6 THEN $7 ELSE department_id END, status = COALESCE($8,status), default_repository_id = CASE WHEN $9 THEN $10 ELSE default_repository_id END, default_environment_id = CASE WHEN $11 THEN $12 ELSE default_environment_id END, notification_policy = COALESCE($13,notification_policy), quality_gate_template = COALESCE($14,quality_gate_template), archived_at = CASE WHEN $8 = 'archived' THEN COALESCE(archived_at,now()) WHEN $8 IS NOT NULL THEN NULL ELSE archived_at END, updated_at=now() WHERE id=$1 AND organization_id=$2 AND ($15='all' OR $15='organization' OR ($15='self' AND owner_user_id=$16) OR ($15='department' AND department_id=$17) OR ($15='department_and_children' AND department_id IN (SELECT id FROM visible_departments)))")
        .bind(id).bind(actor.organization_id).bind(u.name).bind(u.description_set).bind(u.description).bind(u.department_set).bind(u.department_id).bind(u.status).bind(u.default_repository_set).bind(u.default_repository_id).bind(u.default_environment_set).bind(u.default_environment_id).bind(u.notification_policy).bind(u.quality_gate_template).bind(actor.data_scope.as_str()).bind(actor.user_id).bind(actor.department_id).execute(c).await?;
    Ok(result.rows_affected() > 0)
}

pub async fn archive_project(
    c: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$5 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) UPDATE devrail_projects SET status='archived', archived_at=now(), updated_at=now() WHERE id=$1 AND organization_id=$2 AND ($3='all' OR $3='organization' OR ($3='self' AND owner_user_id=$4) OR ($3='department' AND department_id=$5) OR ($3='department_and_children' AND department_id IN (SELECT id FROM visible_departments))) AND archived_at IS NULL").bind(id).bind(actor.organization_id).bind(actor.data_scope.as_str()).bind(actor.user_id).bind(actor.department_id).execute(c).await?;
    Ok(result.rows_affected() > 0)
}

async fn child_scope_sql(table: &str, alias: &str, include_project: bool) -> String {
    let project = if include_project {
        format!(" AND {alias}.project_id = $5")
    } else {
        String::new()
    };
    format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id = $4 AND organization_id = $2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id = parent.id WHERE child.organization_id = $2) SELECT {columns} FROM {table} {alias} WHERE {scope}{project} AND {alias}.archived_at IS NULL", columns = match table { "devrail_repositories" => REPOSITORY_COLUMNS, "devrail_environments" => ENVIRONMENT_COLUMNS, _ => TASK_COLUMNS }, scope = scope_sql(alias))
}

pub async fn list_repositories(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
    page: i64,
    size: i64,
) -> Result<Vec<DevRailRepositoryRow>, sqlx::Error> {
    let sql = format!("{} AND ($6::text IS NULL OR r.name ILIKE '%' || $6 || '%') ORDER BY r.updated_at DESC, r.id DESC LIMIT $7 OFFSET $8", child_scope_sql("devrail_repositories", "r", true).await);
    sqlx::query_as::<_, DevRailRepositoryRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(q.project_id)
        .bind(q.keyword.as_deref())
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(pool)
        .await
}
pub async fn count_repositories(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<i64, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT count(*) FROM devrail_repositories r WHERE {} AND r.project_id=$5 AND r.archived_at IS NULL AND ($6::text IS NULL OR r.name ILIKE '%' || $6 || '%')", scope_sql("r"));
    sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(q.project_id)
        .bind(q.keyword.as_deref())
        .fetch_one(pool)
        .await
}
pub async fn find_repository(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<Option<DevRailRepositoryRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {REPOSITORY_COLUMNS} FROM devrail_repositories r WHERE r.id=$5 AND r.project_id=$6 AND r.archived_at IS NULL AND {}", scope_sql("r"));
    sqlx::query_as::<_, DevRailRepositoryRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
}
pub(crate) async fn create_repository(
    c: &mut PgConnection,
    actor: &ActorContext,
    n: &NewRepository<'_>,
) -> Result<DevRailRepositoryRow, sqlx::Error> {
    let sql=format!("INSERT INTO devrail_repositories (organization_id,department_id,owner_user_id,project_id,name,remote_url,protocol,default_branch,credential_ref) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING {REPOSITORY_COLUMNS}");
    sqlx::query_as::<_, DevRailRepositoryRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(n.department_id)
        .bind(actor.user_id)
        .bind(n.project_id)
        .bind(n.name)
        .bind(n.remote_url)
        .bind(n.protocol)
        .bind(n.default_branch)
        .bind(n.credential_ref)
        .fetch_one(c)
        .await
}
pub(crate) async fn update_repository(
    c: &mut PgConnection,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    u: &RepositoryUpdate<'_>,
) -> Result<bool, sqlx::Error> {
    let r=sqlx::query("UPDATE devrail_repositories SET name=COALESCE($5,name),remote_url=COALESCE($6,remote_url),protocol=COALESCE($7,protocol),default_branch=COALESCE($8,default_branch),credential_ref=CASE WHEN $9 THEN $10 ELSE credential_ref END,archived_at=CASE WHEN $11='archived' THEN COALESCE(archived_at,now()) WHEN $11 IS NOT NULL THEN NULL ELSE archived_at END,updated_at=now() WHERE id=$1 AND project_id=$2 AND organization_id=$3 AND ($4='all' OR owner_user_id=$12 OR $4='organization' OR ($4 IN ('department','department_and_children') AND department_id=$13)) AND archived_at IS NULL").bind(id).bind(project_id).bind(actor.organization_id).bind(actor.data_scope.as_str()).bind(u.name).bind(u.remote_url).bind(u.protocol).bind(u.default_branch).bind(u.credential_set).bind(u.credential_ref).bind(u.status).bind(actor.user_id).bind(actor.department_id).execute(c).await?;
    Ok(r.rows_affected() > 0)
}

pub(crate) async fn update_repository_sync(
    c: &mut PgConnection,
    actor: &ActorContext,
    input: &RepositorySyncUpdate<'_>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE devrail_repositories SET last_sync_status=$5,last_head_sha=$6,last_remote_branch=$7,last_remote_branch_count=$8,updated_at=now() WHERE id=$1 AND project_id=$2 AND organization_id=$3 AND ($4='all' OR owner_user_id=$9 OR $4='organization' OR ($4 IN ('department','department_and_children') AND department_id=$10)) AND archived_at IS NULL")
        .bind(input.id).bind(input.project_id).bind(actor.organization_id).bind(actor.data_scope.as_str()).bind(input.status).bind(input.head_sha).bind(input.remote_branch).bind(input.remote_branch_count).bind(actor.user_id).bind(actor.department_id).execute(c).await?;
    Ok(result.rows_affected() == 1)
}

pub async fn list_environments(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
    page: i64,
    size: i64,
) -> Result<Vec<DevRailEnvironmentRow>, sqlx::Error> {
    let sql=format!("{} AND ($6::text IS NULL OR e.name ILIKE '%' || $6 || '%') ORDER BY e.updated_at DESC,e.id DESC LIMIT $7 OFFSET $8",child_scope_sql("devrail_environments","e",true).await);
    sqlx::query_as::<_, DevRailEnvironmentRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(q.project_id)
        .bind(q.keyword.as_deref())
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(pool)
        .await
}
pub async fn count_environments(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<i64, sqlx::Error> {
    let sql=format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT count(*) FROM devrail_environments e WHERE {} AND e.project_id=$5 AND e.archived_at IS NULL AND ($6::text IS NULL OR e.name ILIKE '%' || $6 || '%')",scope_sql("e"));
    sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(q.project_id)
        .bind(q.keyword.as_deref())
        .fetch_one(pool)
        .await
}
pub async fn find_environment(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<Option<DevRailEnvironmentRow>, sqlx::Error> {
    let sql=format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {ENVIRONMENT_COLUMNS} FROM devrail_environments e WHERE e.id=$5 AND e.project_id=$6 AND e.archived_at IS NULL AND {}",scope_sql("e"));
    sqlx::query_as::<_, DevRailEnvironmentRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
}
pub(crate) async fn create_environment(
    c: &mut PgConnection,
    actor: &ActorContext,
    n: &NewEnvironment<'_>,
) -> Result<DevRailEnvironmentRow, sqlx::Error> {
    let sql=format!("INSERT INTO devrail_environments (organization_id,department_id,owner_user_id,project_id,name,workspace_root,network_mode,tool_policy,secret_refs,max_duration_secs,enabled) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING {ENVIRONMENT_COLUMNS}");
    sqlx::query_as::<_, DevRailEnvironmentRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(n.department_id)
        .bind(actor.user_id)
        .bind(n.project_id)
        .bind(n.name)
        .bind(n.workspace_root)
        .bind(n.network_mode)
        .bind(n.tool_policy)
        .bind(n.secret_refs)
        .bind(n.max_duration_secs)
        .bind(n.enabled)
        .fetch_one(c)
        .await
}
pub(crate) async fn update_environment(
    c: &mut PgConnection,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    u: &EnvironmentUpdate<'_>,
) -> Result<bool, sqlx::Error> {
    let r=sqlx::query("UPDATE devrail_environments SET name=COALESCE($5,name),workspace_root=COALESCE($6,workspace_root),network_mode=COALESCE($7,network_mode),tool_policy=COALESCE($8,tool_policy),secret_refs=COALESCE($9,secret_refs),max_duration_secs=COALESCE($10,max_duration_secs),enabled=COALESCE($11,enabled),updated_at=now() WHERE id=$1 AND project_id=$2 AND organization_id=$3 AND ($4='all' OR owner_user_id=$12 OR $4='organization' OR ($4 IN ('department','department_and_children') AND department_id=$13)) AND archived_at IS NULL").bind(id).bind(project_id).bind(actor.organization_id).bind(actor.data_scope.as_str()).bind(u.name).bind(u.workspace_root).bind(u.network_mode).bind(u.tool_policy).bind(u.secret_refs).bind(u.max_duration_secs).bind(u.enabled).bind(actor.user_id).bind(actor.department_id).execute(c).await?;
    Ok(r.rows_affected() > 0)
}

pub async fn list_tasks(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
    page: i64,
    size: i64,
) -> Result<Vec<DevRailTaskRow>, sqlx::Error> {
    let sql=format!("{} AND ($6::text IS NULL OR t.title ILIKE '%' || $6 || '%' OR t.goal ILIKE '%' || $6 || '%') AND ($7::text IS NULL OR t.status=$7) AND ($8::bigint IS NULL OR t.assignee_user_id=$8) AND ($9::text IS NULL OR t.labels @> jsonb_build_array($9::text)) ORDER BY t.updated_at DESC,t.id DESC LIMIT $10 OFFSET $11",child_scope_sql("devrail_tasks","t",true).await);
    sqlx::query_as::<_, DevRailTaskRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(q.project_id)
        .bind(q.keyword.as_deref())
        .bind(q.status.as_deref())
        .bind(q.assignee_user_id)
        .bind(q.label.as_deref())
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(pool)
        .await
}
pub async fn count_tasks(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<i64, sqlx::Error> {
    let sql=format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT count(*) FROM devrail_tasks t WHERE {} AND t.project_id=$5 AND t.archived_at IS NULL AND ($6::text IS NULL OR t.title ILIKE '%' || $6 || '%' OR t.goal ILIKE '%' || $6 || '%') AND ($7::text IS NULL OR t.status=$7) AND ($8::bigint IS NULL OR t.assignee_user_id=$8) AND ($9::text IS NULL OR t.labels @> jsonb_build_array($9::text))",scope_sql("t"));
    sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(q.project_id)
        .bind(q.keyword.as_deref())
        .bind(q.status.as_deref())
        .bind(q.assignee_user_id)
        .bind(q.label.as_deref())
        .fetch_one(pool)
        .await
}
pub async fn find_task(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<Option<DevRailTaskRow>, sqlx::Error> {
    let sql=format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {TASK_COLUMNS} FROM devrail_tasks t WHERE t.id=$5 AND t.project_id=$6 AND t.archived_at IS NULL AND {}",scope_sql("t"));
    sqlx::query_as::<_, DevRailTaskRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
}

pub async fn find_task_by_id(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailTaskRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {TASK_COLUMNS} FROM devrail_tasks t WHERE t.id=$5 AND t.archived_at IS NULL AND {}", scope_sql("t"));
    sqlx::query_as::<_, DevRailTaskRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Returns queued tasks whose environment is enabled and atomically claims
/// them for one scheduler tick. The row lock and `SKIP LOCKED` keep multiple
/// backend instances from selecting the same task while the claim lease lets
/// a restarted scheduler recover abandoned work.
pub(crate) async fn claim_scheduler_tasks(
    pool: &PgPool,
    claim_token: uuid::Uuid,
    limit: i64,
    claim_lease_seconds: i64,
    priority_aging_seconds: i64,
) -> Result<Vec<DevRailTaskRow>, sqlx::Error> {
    let sql = format!(
        r#"WITH candidates AS (
            SELECT t.id,
                   GREATEST(
                       COALESCE((SELECT MAX(r.attempt) FROM devrail_runs r WHERE r.task_id = t.id), 0) + 1,
                       1
                   ) AS next_attempt
            FROM devrail_tasks t
            JOIN devrail_environments e
              ON e.id = t.environment_id
             AND e.organization_id = t.organization_id
             AND e.project_id = t.project_id
             AND e.owner_user_id = t.owner_user_id
             AND e.department_id IS NOT DISTINCT FROM t.department_id
            WHERE t.status = 'queued'
              AND t.archived_at IS NULL
              AND t.scheduler_attempt < t.scheduler_max_attempts
              AND e.enabled
              AND e.archived_at IS NULL
              AND (
                  t.scheduler_claimed_at IS NULL
                  OR t.scheduler_claimed_at < now() - make_interval(secs => $3)
              )
              AND (t.scheduler_retry_at IS NULL OR t.scheduler_retry_at <= now())
              AND NOT EXISTS (
                  SELECT 1 FROM devrail_runs r
                  WHERE r.task_id = t.id
                    AND r.status IN ('starting','active','awaiting_approval')
              )
            ORDER BY
                CASE t.priority
                    WHEN 'urgent' THEN 0
                    WHEN 'high' THEN 1
                    WHEN 'normal' THEN 2
                    ELSE 3
                END - LEAST(
                    3,
                    FLOOR(EXTRACT(EPOCH FROM (now() - t.created_at)) / $4)::integer
                ),
                t.due_at ASC NULLS LAST,
                t.created_at ASC,
                t.id ASC
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE devrail_tasks t
        SET scheduler_claim_token = $2,
            scheduler_claimed_at = now(),
            scheduler_attempt = candidates.next_attempt,
            updated_at = now()
        FROM candidates
        WHERE t.id = candidates.id
        RETURNING {SCHEDULER_TASK_COLUMNS}"#
    );
    sqlx::query_as::<_, DevRailTaskRow>(AssertSqlSafe(sql))
        .bind(limit)
        .bind(claim_token)
        .bind(claim_lease_seconds)
        .bind(priority_aging_seconds)
        .fetch_all(pool)
        .await
}

pub(crate) async fn release_scheduler_claim(
    pool: &PgPool,
    task_id: i64,
    claim_token: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_tasks SET scheduler_claim_token = NULL, scheduler_claimed_at = NULL, updated_at = now() WHERE id = $1 AND status = 'queued' AND scheduler_claim_token = $2",
    )
    .bind(task_id)
    .bind(claim_token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Extends a scheduler lease only when the caller still owns its token. A
/// zero-row update is an explicit stale-worker signal and must stop further
/// writes for that task.
pub(crate) async fn renew_scheduler_claim(
    pool: &PgPool,
    task_id: i64,
    claim_token: uuid::Uuid,
    claim_lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_tasks
         SET scheduler_claimed_at = now(), updated_at = now()
         WHERE id = $1 AND status = 'queued' AND scheduler_claim_token = $2
           AND scheduler_claimed_at >= now() - make_interval(secs => $3)",
    )
    .bind(task_id)
    .bind(claim_token)
    .bind(claim_lease_seconds)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn scheduler_claim_is_current(
    c: &mut PgConnection,
    task_id: i64,
    claim_token: uuid::Uuid,
    claim_lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT id FROM devrail_tasks
         WHERE id = $1 AND status = 'queued'
           AND scheduler_claim_token = $2
           AND scheduler_claimed_at >= now() - make_interval(secs => $3)
         FOR UPDATE",
    )
    .bind(task_id)
    .bind(claim_token)
    .bind(claim_lease_seconds)
    .fetch_optional(c)
    .await
    .map(|row| row.is_some())
}

#[derive(Debug, Clone)]
pub(crate) struct PendingRunInterruption {
    pub run_id: i64,
    pub reason: String,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct SchedulerReconciliation {
    pub queue_depth: i64,
    pub active_runs: i64,
    pub released_claims: u64,
    pub stale_runs: u64,
    pub exhausted_tasks: u64,
    pub pending_interruptions: Vec<PendingRunInterruption>,
}

pub(crate) async fn reconcile_scheduler_state(
    pool: &PgPool,
    running_run_ids: &[i64],
    stale_timeout_seconds: i64,
) -> Result<SchedulerReconciliation, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let released_claims = sqlx::query(
        "UPDATE devrail_tasks
         SET scheduler_claim_token = NULL, scheduler_claimed_at = NULL, updated_at = now()
         WHERE status <> 'queued' AND scheduler_claim_token IS NOT NULL",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let exhausted_tasks = sqlx::query(
        "UPDATE devrail_tasks
         SET status = 'failed', scheduler_claim_token = NULL,
             scheduler_claimed_at = NULL,
             scheduler_last_error = '已达到调度最大尝试次数', updated_at = now()
         WHERE status = 'queued' AND archived_at IS NULL
           AND scheduler_attempt >= scheduler_max_attempts
           AND (scheduler_retry_at IS NULL OR scheduler_retry_at <= now())",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let pending_interruptions = sqlx::query_as::<_, (i64, String)>(
        "SELECT r.id,
                CASE WHEN t.status = 'cancelled'
                     THEN 'task_cancelled'
                     ELSE 'environment_invalid'
                END AS reason
         FROM devrail_runs r
         JOIN devrail_tasks t ON t.id = r.task_id
         LEFT JOIN devrail_environments e ON e.id = t.environment_id
         WHERE r.status IN ('starting','active')
           AND (
               t.status = 'cancelled'
               OR (
                   r.status = 'starting'
                   AND (t.archived_at IS NOT NULL OR e.id IS NULL OR NOT e.enabled OR e.archived_at IS NOT NULL)
               )
           )
         ORDER BY r.id",
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|(run_id, reason)| PendingRunInterruption { run_id, reason })
    .collect::<Vec<_>>();
    if !pending_interruptions.is_empty() {
        let interruption_ids = pending_interruptions
            .iter()
            .map(|pending| pending.run_id)
            .collect::<Vec<_>>();
        let interruption_trace = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audit_logs
                 (actor_user_id, action, target_type, target_id, details, trace_id,
                  organization_id, department_id)
             SELECT NULL, 'devrail.run.reconcile_interrupt', 'devrail_run', r.id,
                    jsonb_build_object(
                        'actorType', 'system',
                        'reason', CASE WHEN t.status='cancelled'
                                       THEN 'task_cancelled'
                                       ELSE 'environment_invalid' END,
                        'policyVersion', 'devrail-policy-v1'
                    ), $2, r.organization_id, r.department_id
             FROM devrail_runs r
             JOIN devrail_tasks t ON t.id=r.task_id
             WHERE r.id=ANY($1::bigint[])
               AND NOT EXISTS (
                   SELECT 1 FROM audit_logs a
                   WHERE a.action='devrail.run.reconcile_interrupt'
                     AND a.target_type='devrail_run' AND a.target_id=r.id
               )",
        )
        .bind(&interruption_ids)
        .bind(interruption_trace)
        .execute(&mut *tx)
        .await?;
    }
    let stale_run_ids = sqlx::query_scalar::<_, i64>(
        "UPDATE devrail_runs AS r
         SET status = 'failed', exit_reason = 'supervisor_process_missing',
             retry_reason = 'Supervisor 对账未发现对应进程',
             recovery_suggestion = '运行进程已退出；请检查日志后重试',
             completed_at = COALESCE(r.completed_at, now()), updated_at = now(),
             cleanup_status = 'completed'
         WHERE r.status IN ('starting', 'active')
           AND COALESCE(r.last_heartbeat_at, r.last_event_at, r.updated_at)
               < now() - make_interval(secs => $2)
           AND NOT (r.id = ANY($1::bigint[]))
         RETURNING r.id",
    )
    .bind(running_run_ids)
    .bind(stale_timeout_seconds)
    .fetch_all(&mut *tx)
    .await?;
    let stale_runs = stale_run_ids.len() as u64;
    if stale_runs > 0 {
        sqlx::query(
            "UPDATE devrail_tasks AS t
             SET status = 'failed', scheduler_claim_token = NULL,
                 scheduler_claimed_at = NULL, updated_at = now()
             WHERE t.status = 'running'
               AND EXISTS (
                   SELECT 1 FROM devrail_runs r
                   WHERE r.task_id = t.id
                     AND r.status = 'failed'
                     AND r.exit_reason = 'supervisor_process_missing'
               )",
        )
        .execute(&mut *tx)
        .await?;
        let reconciliation_trace = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audit_logs
                 (actor_user_id, action, target_type, target_id, details, trace_id,
                  organization_id, department_id)
             SELECT NULL, 'devrail.run.reconcile', 'devrail_run', r.id,
                    jsonb_build_object(
                        'actorType', 'system',
                        'reason', 'supervisor_process_missing',
                        'policyVersion', 'devrail-policy-v1'
                    ), $2, r.organization_id, r.department_id
             FROM devrail_runs r
             WHERE r.id=ANY($1::bigint[])",
        )
        .bind(&stale_run_ids)
        .bind(reconciliation_trace)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO devrail_notifications
                 (organization_id, department_id, recipient_user_id, event_type,
                  level, title, summary, resource_type, resource_id, deep_link, source_key)
             SELECT r.organization_id, r.department_id, r.owner_user_id,
                    'run.failed', 'error', '运行恢复失败',
                    'Supervisor 对账未发现对应进程，请检查日志后重试',
                    'devrail_run', r.id, '/devrail/runs/' || r.id,
                    'run:' || r.id || ':supervisor_process_missing'
             FROM devrail_runs r
             WHERE r.status='failed' AND r.exit_reason='supervisor_process_missing'
             ON CONFLICT (recipient_user_id, source_key) DO NOTHING",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO devrail_outbox_events
                 (organization_id, event_type, aggregate_type, aggregate_id, payload)
             SELECT r.organization_id, 'notification.created', 'devrail_run', r.id,
                    jsonb_build_object(
                        'notificationSource',
                        'run:' || r.id || ':supervisor_process_missing'
                    )
             FROM devrail_runs r
             WHERE r.status='failed' AND r.exit_reason='supervisor_process_missing'
             ON CONFLICT DO NOTHING",
        )
        .execute(&mut *tx)
        .await?;
    }
    let queue_depth = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM devrail_tasks
         WHERE status = 'queued' AND archived_at IS NULL",
    )
    .fetch_one(&mut *tx)
    .await?;
    let active_runs = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM devrail_runs
         WHERE status IN ('starting','active','awaiting_approval')",
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(SchedulerReconciliation {
        queue_depth,
        active_runs,
        released_claims,
        stale_runs,
        exhausted_tasks,
        pending_interruptions,
    })
}

pub(crate) async fn schedule_retry(
    pool: &PgPool,
    task_id: i64,
    claim_token: uuid::Uuid,
    retry_at: chrono::DateTime<chrono::Utc>,
    error: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_tasks
         SET scheduler_claim_token = NULL, scheduler_claimed_at = NULL,
             scheduler_retry_count = scheduler_retry_count + 1,
             scheduler_retry_at = $3, scheduler_last_error = $4,
             status = CASE WHEN scheduler_retry_count + 1 >= scheduler_max_attempts
                           THEN 'failed' ELSE status END,
             updated_at = now()
         WHERE id = $1 AND status = 'queued' AND scheduler_claim_token = $2",
    )
    .bind(task_id)
    .bind(claim_token)
    .bind(retry_at)
    .bind(error.chars().take(500).collect::<String>())
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn fail_scheduler_task(
    pool: &PgPool,
    task_id: i64,
    claim_token: uuid::Uuid,
    error: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_tasks
         SET status = 'failed', scheduler_claim_token = NULL,
             scheduler_claimed_at = NULL, scheduler_last_error = $3,
             updated_at = now()
         WHERE id = $1 AND status = 'queued' AND scheduler_claim_token = $2",
    )
    .bind(task_id)
    .bind(claim_token)
    .bind(error.chars().take(500).collect::<String>())
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn create_task(
    c: &mut PgConnection,
    actor: &ActorContext,
    n: &NewTask<'_>,
) -> Result<DevRailTaskRow, sqlx::Error> {
    let sql=format!("INSERT INTO devrail_tasks (organization_id,department_id,owner_user_id,project_id,repository_id,environment_id,assignee_user_id,title,goal,background,acceptance_criteria,constraints,priority,labels,due_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING {TASK_COLUMNS}");
    sqlx::query_as::<_, DevRailTaskRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(n.department_id)
        .bind(actor.user_id)
        .bind(n.project_id)
        .bind(n.repository_id)
        .bind(n.environment_id)
        .bind(n.assignee_user_id)
        .bind(n.title)
        .bind(n.goal)
        .bind(n.background)
        .bind(n.acceptance_criteria)
        .bind(n.constraints)
        .bind(n.priority)
        .bind(n.labels)
        .bind(n.due_at)
        .fetch_one(c)
        .await
}
pub(crate) async fn update_task(
    c: &mut PgConnection,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    u: &TaskUpdate<'_>,
) -> Result<bool, sqlx::Error> {
    if u.status.is_some() {
        sqlx::query(
            "SELECT set_config('devrail.actor_type',$1,true),
                    set_config('devrail.actor_user_id',$2,true),
                    set_config('devrail.transition_reason',$3,true),
                    set_config('devrail.trace_id',$4,true)",
        )
        .bind(actor.actor_type.as_str())
        .bind(actor.user_id.to_string())
        .bind("task_api_update")
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&mut *c)
        .await?;
    }
    let r = sqlx::query(
        "UPDATE devrail_tasks
         SET title=COALESCE($5,title), goal=COALESCE($6,goal),
             background=CASE WHEN $7 THEN $8 ELSE background END,
             acceptance_criteria=CASE WHEN $9 THEN $10 ELSE acceptance_criteria END,
             constraints=CASE WHEN $11 THEN $12 ELSE constraints END,
             priority=COALESCE($13,priority), status=COALESCE($14,status),
             assignee_user_id=CASE WHEN $15 THEN $16 ELSE assignee_user_id END,
             labels=COALESCE($17,labels),
             due_at=CASE WHEN $18 THEN $19 ELSE due_at END,
             repository_id=CASE WHEN $20 THEN $21 ELSE repository_id END,
             environment_id=CASE WHEN $22 THEN $23 ELSE environment_id END,
             dispatch_snapshot=CASE WHEN $14='queued' THEN COALESCE($26,dispatch_snapshot) ELSE dispatch_snapshot END,
             dispatch_snapshot_digest=CASE WHEN $14='queued' THEN COALESCE($27,dispatch_snapshot_digest) ELSE dispatch_snapshot_digest END,
             workflow_source=CASE WHEN $14='queued' THEN COALESCE($28,workflow_source) ELSE workflow_source END,
             workflow_version=CASE WHEN $14='queued' THEN COALESCE($29,workflow_version) ELSE workflow_version END,
             workflow_digest=CASE WHEN $14='queued' THEN COALESCE($30,workflow_digest) ELSE workflow_digest END,
             scheduler_max_attempts=CASE WHEN $14='queued' THEN COALESCE($31,scheduler_max_attempts) ELSE scheduler_max_attempts END,
             archived_at=CASE
                 WHEN $14='archived' THEN COALESCE(archived_at,now())
                 WHEN $14 IS NOT NULL THEN NULL ELSE archived_at END,
             updated_at=now()
         WHERE id=$1 AND project_id=$2 AND organization_id=$3
           AND ($4='all' OR owner_user_id=$24 OR $4='organization'
                OR ($4 IN ('department','department_and_children') AND department_id=$25))
           AND archived_at IS NULL
           AND ($14::text IS NULL OR status=$14 OR
                (status='draft' AND $14 IN ('queued','cancelled','archived')) OR
                (status='queued' AND $14 IN ('cancelled','failed')) OR
                (status='running' AND $14 IN ('awaiting_approval','succeeded','failed','cancelled')) OR
                (status='awaiting_approval' AND $14 IN ('running','succeeded','failed','cancelled')) OR
                (status IN ('succeeded','failed','cancelled') AND $14='archived') OR
                (status='failed' AND $14='queued'))",
    )
    .bind(id)
    .bind(project_id)
    .bind(actor.organization_id)
    .bind(actor.data_scope.as_str())
    .bind(u.title)
    .bind(u.goal)
    .bind(u.background_set)
    .bind(u.background)
    .bind(u.acceptance_set)
    .bind(u.acceptance_criteria)
    .bind(u.constraints_set)
    .bind(u.constraints)
    .bind(u.priority)
    .bind(u.status)
    .bind(u.assignee_set)
    .bind(u.assignee_user_id)
    .bind(u.labels)
    .bind(u.due_at_set)
    .bind(u.due_at)
    .bind(u.repository_set)
    .bind(u.repository_id)
    .bind(u.environment_set)
    .bind(u.environment_id)
    .bind(actor.user_id)
    .bind(actor.department_id)
    .bind(u.queue_snapshot)
    .bind(u.queue_snapshot_digest)
    .bind(u.workflow_source)
    .bind(u.workflow_version)
    .bind(u.workflow_digest)
    .bind(u.queue_max_attempts)
    .execute(c)
    .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod scheduler_integration_tests {
    use super::*;
    use crate::access::{ActorType, DataScope};
    use crate::db::DATABASE_TEST_LOCK;
    use crate::repositories::devrail_runs;
    use serde_json::json;
    use std::collections::BTreeSet;
    use uuid::Uuid;

    async fn test_pool() -> Option<PgPool> {
        let database_url = std::env::var("TEST_DATABASE_URL").ok()?;
        let pool = crate::db::init_pool(&database_url).await.ok()?;
        crate::db::run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    async fn scheduler_fixture(pool: &PgPool) -> (i64, i64, i64, Option<i64>) {
        let (owner_user_id, organization_id, department_id) =
            sqlx::query_as::<_, (i64, i64, Option<i64>)>(
                "SELECT id, organization_id, department_id FROM users ORDER BY id LIMIT 1",
            )
            .fetch_one(pool)
            .await
            .expect("seeded user");
        let suffix = Uuid::new_v4().simple().to_string();
        let project_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_projects
                 (organization_id, department_id, owner_user_id, slug, name)
             VALUES ($1,$2,$3,$4,$5) RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(format!("scheduler-{suffix}"))
        .bind("调度集成测试")
        .fetch_one(pool)
        .await
        .expect("create project");
        let environment_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_environments
                 (organization_id, department_id, owner_user_id, project_id,
                  name, workspace_root)
             VALUES ($1,$2,$3,$4,$5,'/tmp/devrail-scheduler-test') RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(project_id)
        .bind(format!("environment-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("create environment");
        (project_id, environment_id, owner_user_id, department_id)
    }

    async fn queued_task(
        pool: &PgPool,
        fixture: (i64, i64, i64, Option<i64>),
        max_attempts: i32,
    ) -> i64 {
        let (project_id, environment_id, owner_user_id, department_id) = fixture;
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_tasks
                 (organization_id, department_id, owner_user_id, project_id,
                  environment_id, title, goal, status, scheduler_max_attempts)
             SELECT organization_id,$2,$3,id,$4,$5,'验证调度可靠性','queued',$6
             FROM devrail_projects WHERE id=$1 RETURNING id",
        )
        .bind(project_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(environment_id)
        .bind(format!("调度任务-{}", Uuid::new_v4().simple()))
        .bind(max_attempts)
        .fetch_one(pool)
        .await
        .expect("create queued task")
    }

    #[tokio::test]
    async fn queued_snapshot_is_atomic_immutable_and_audited() {
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let fixture = scheduler_fixture(&pool).await;
        let (project_id, environment_id, owner_user_id, department_id) = fixture;
        let organization_id = sqlx::query_scalar::<_, i64>(
            "SELECT organization_id FROM devrail_projects WHERE id=$1",
        )
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .expect("project organization");
        let task_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_tasks
                 (organization_id, department_id, owner_user_id, project_id,
                  environment_id, title, goal)
             VALUES ($1,$2,$3,$4,$5,'快照任务','验证不可变快照') RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(project_id)
        .bind(environment_id)
        .fetch_one(&pool)
        .await
        .expect("create draft task");
        let actor = ActorContext {
            actor_type: ActorType::User,
            user_id: owner_user_id,
            session_id: 1,
            organization_id,
            department_id,
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        };
        let dispatch_snapshot = json!({
            "schemaVersion": 1,
            "taskRevision": 1,
            "workflow": {
                "source": "repository",
                "declaredVersion": "v1",
                "digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        });
        let mut tx = pool.begin().await.expect("begin queue transition");
        assert!(update_task(
            &mut tx,
            &actor,
            project_id,
            task_id,
            &TaskUpdate {
                title: None,
                goal: None,
                background_set: false,
                background: None,
                acceptance_set: false,
                acceptance_criteria: None,
                constraints_set: false,
                constraints: None,
                priority: None,
                status: Some("queued"),
                assignee_set: false,
                assignee_user_id: None,
                labels: None,
                due_at_set: false,
                due_at: None,
                repository_set: false,
                repository_id: None,
                environment_set: false,
                environment_id: None,
                queue_snapshot: Some(&dispatch_snapshot),
                queue_snapshot_digest: Some(
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                ),
                workflow_source: Some("repository"),
                workflow_version: Some("v1"),
                workflow_digest: Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
                queue_max_attempts: Some(4),
            },
        )
        .await
        .expect("queue task"));
        tx.commit().await.expect("commit queue transition");
        let stored = sqlx::query_as::<_, (String, i64, String, String, i32)>(
            "SELECT status, revision, workflow_source, workflow_digest,
                    scheduler_max_attempts
             FROM devrail_tasks WHERE id=$1",
        )
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("queued snapshot identity");
        assert_eq!(stored.0, "queued");
        assert_eq!(stored.1, 1);
        assert_eq!(stored.2, "repository");
        assert_eq!(stored.3, "a".repeat(64));
        assert_eq!(stored.4, 4);
        let history = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT from_status, to_status, reason, actor_user_id
             FROM devrail_task_status_history WHERE task_id=$1",
        )
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("status history");
        assert_eq!(history.0, "draft");
        assert_eq!(history.1, "queued");
        assert_eq!(history.2, "task_api_update");
        assert_eq!(history.3, owner_user_id);
        assert!(
            sqlx::query("UPDATE devrail_tasks SET title='漂移标题' WHERE id=$1")
                .bind(task_id)
                .execute(&pool)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn concurrent_claim_lease_expiry_cancel_and_retry_limit_are_deterministic() {
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let fixture = scheduler_fixture(&pool).await;
        let task_id = queued_task(&pool, fixture, 3).await;
        let token_a = Uuid::new_v4();
        let token_b = Uuid::new_v4();
        let (claimed_a, claimed_b) = tokio::join!(
            claim_scheduler_tasks(&pool, token_a, 100, 60, 3_600),
            claim_scheduler_tasks(&pool, token_b, 100, 60, 3_600)
        );
        let claimed_a = claimed_a.expect("worker A claim");
        let claimed_b = claimed_b.expect("worker B claim");
        let worker_a_claimed_task = claimed_a.iter().any(|task| task.id == task_id);
        let worker_b_claimed_task = claimed_b.iter().any(|task| task.id == task_id);
        assert_ne!(worker_a_claimed_task, worker_b_claimed_task);
        let old_token = if worker_a_claimed_task {
            token_a
        } else {
            token_b
        };

        sqlx::query(
            "UPDATE devrail_tasks SET scheduler_claimed_at=now()-INTERVAL '61 seconds'
             WHERE id=$1",
        )
        .bind(task_id)
        .execute(&pool)
        .await
        .expect("expire lease");
        let replacement = Uuid::new_v4();
        assert!(claim_scheduler_tasks(&pool, replacement, 100, 60, 3_600)
            .await
            .expect("replacement claim")
            .iter()
            .any(|task| task.id == task_id));
        assert!(!renew_scheduler_claim(&pool, task_id, old_token, 60)
            .await
            .expect("reject stale owner"));
        sqlx::query("UPDATE devrail_tasks SET status='cancelled' WHERE id=$1")
            .bind(task_id)
            .execute(&pool)
            .await
            .expect("cancel task");
        assert!(!renew_scheduler_claim(&pool, task_id, replacement, 60)
            .await
            .expect("cancel invalidates claim"));

        let limited_task = queued_task(&pool, fixture, 1).await;
        let limited_token = Uuid::new_v4();
        assert!(claim_scheduler_tasks(&pool, limited_token, 100, 60, 3_600)
            .await
            .expect("limited claim")
            .iter()
            .any(|task| task.id == limited_task));
        assert!(schedule_retry(
            &pool,
            limited_task,
            limited_token,
            chrono::Utc::now(),
            "temporary transport failure",
        )
        .await
        .expect("schedule retry"));
        let status =
            sqlx::query_scalar::<_, String>("SELECT status FROM devrail_tasks WHERE id=$1")
                .bind(limited_task)
                .fetch_one(&pool)
                .await
                .expect("limited status");
        assert_eq!(status, "failed");

        let aged_low = queued_task(&pool, fixture, 3).await;
        let fresh_urgent = queued_task(&pool, fixture, 3).await;
        sqlx::query(
            "UPDATE devrail_tasks
             SET priority='low', created_at=now()-INTERVAL '4 hours',
                 due_at=now()-INTERVAL '1 year'
             WHERE id=$1",
        )
        .bind(aged_low)
        .execute(&pool)
        .await
        .expect("age low-priority task");
        sqlx::query("UPDATE devrail_tasks SET priority='urgent' WHERE id=$1")
            .bind(fresh_urgent)
            .execute(&pool)
            .await
            .expect("mark urgent task");
        let aging_token = Uuid::new_v4();
        let aging_claim = claim_scheduler_tasks(&pool, aging_token, 1, 60, 3_600)
            .await
            .expect("claim with priority aging");
        assert_eq!(aging_claim.len(), 1);
        assert_eq!(aging_claim[0].id, aged_low);
        sqlx::query(
            "UPDATE devrail_tasks
             SET status='cancelled', scheduler_claim_token=NULL, scheduler_claimed_at=NULL
             WHERE id = ANY($1::bigint[])",
        )
        .bind(vec![aged_low, fresh_urgent])
        .execute(&pool)
        .await
        .expect("clean priority aging tasks");
    }

    #[tokio::test]
    async fn reconciliation_terminal_transition_and_notification_are_idempotent() {
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let fixture = scheduler_fixture(&pool).await;
        let task_id = queued_task(&pool, fixture, 3).await;
        let (project_id, _, owner_user_id, department_id) = fixture;
        let organization_id = sqlx::query_scalar::<_, i64>(
            "SELECT organization_id FROM devrail_projects WHERE id=$1",
        )
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .expect("project organization");
        sqlx::query("UPDATE devrail_tasks SET status='running' WHERE id=$1")
            .bind(task_id)
            .execute(&pool)
            .await
            .expect("mark running");
        let snapshot_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_task_snapshots
                 (organization_id, department_id, owner_user_id, task_id, snapshot)
             VALUES ($1,$2,$3,$4,'{}') RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("create snapshot");
        let actor = ActorContext {
            actor_type: ActorType::System,
            user_id: owner_user_id,
            session_id: 0,
            organization_id,
            department_id,
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        };
        let policy = json!({"version":"test-policy"});
        let startup_args = json!(["app-server"]);
        let workflow_snapshot = json!({"source":"legacy","version":"legacy-v1","digest":"0000000000000000000000000000000000000000000000000000000000000000"});
        let first_key = format!("scheduler:{task_id}:1");
        let mut tx = pool.begin().await.expect("begin run transaction");
        let first_run = devrail_runs::create_run(
            &mut tx,
            &devrail_runs::NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: &first_key,
                attempt: 1,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: "0000000000000000000000000000000000000000000000000000000000000000",
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: None,
                parent_turn_id: None,
                branch_name: None,
                branch_expires_at: None,
                cwd: "/tmp",
                policy: &policy,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("create first run")
        .expect("first run inserted");
        let duplicate = devrail_runs::create_run(
            &mut tx,
            &devrail_runs::NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: &first_key,
                attempt: 1,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: "0000000000000000000000000000000000000000000000000000000000000000",
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: None,
                parent_turn_id: None,
                branch_name: None,
                branch_expires_at: None,
                cwd: "/tmp",
                policy: &policy,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("duplicate insert is deterministic");
        assert!(duplicate.is_none());
        sqlx::query("UPDATE devrail_runs SET status='failed' WHERE id=$1")
            .bind(first_run.id)
            .execute(&mut *tx)
            .await
            .expect("finish parent run");
        let second_key = format!("scheduler:{task_id}:2");
        let stale_run = devrail_runs::create_run(
            &mut tx,
            &devrail_runs::NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: &second_key,
                attempt: 2,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: "0000000000000000000000000000000000000000000000000000000000000000",
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: Some(first_run.id),
                parent_turn_id: Some("turn-parent"),
                branch_name: None,
                branch_expires_at: None,
                cwd: "/tmp",
                policy: &policy,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("create child run")
        .expect("child run inserted");
        assert_eq!(stale_run.parent_run_id, Some(first_run.id));
        assert_eq!(stale_run.parent_turn_id.as_deref(), Some("turn-parent"));
        crate::repositories::audit_logs::record_actor(
            &mut tx,
            &actor,
            "devrail.run.scheduler_test",
            "devrail_run",
            Some(stale_run.id),
            json!({
                "actorType":"system",
                "attempt":stale_run.attempt,
                "reason":"scheduler_dispatch",
                "policyVersion":"test-policy"
            }),
        )
        .await
        .expect("record system actor audit");
        tx.commit().await.expect("commit runs");
        let run_id = stale_run.id;
        let audit = sqlx::query_as::<_, (Option<i64>, Option<String>, serde_json::Value)>(
            "SELECT actor_user_id, trace_id, details FROM audit_logs
             WHERE action='devrail.run.scheduler_test' AND target_id=$1",
        )
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .expect("read system actor audit");
        assert!(audit.0.is_none());
        assert!(audit
            .1
            .is_some_and(|trace| uuid::Uuid::parse_str(&trace).is_ok()));
        assert_eq!(audit.2["actorType"], "system");
        assert_eq!(audit.2["policyVersion"], "test-policy");
        sqlx::query(
            "UPDATE devrail_runs
             SET status='active', updated_at=now()-INTERVAL '61 seconds'
             WHERE id=$1",
        )
        .bind(run_id)
        .execute(&pool)
        .await
        .expect("age stale run");

        sqlx::query("UPDATE devrail_tasks SET status='cancelled' WHERE id=$1")
            .bind(task_id)
            .execute(&pool)
            .await
            .expect("cancel running task");
        let cancellation = reconcile_scheduler_state(&pool, &[run_id], 30)
            .await
            .expect("cancellation reconciliation");
        let run_interruptions = cancellation
            .pending_interruptions
            .iter()
            .filter(|pending| pending.run_id == run_id)
            .collect::<Vec<_>>();
        assert_eq!(run_interruptions.len(), 1);
        assert_eq!(run_interruptions[0].reason, "task_cancelled");
        assert_eq!(cancellation.stale_runs, 0);
        let interruption_audit = sqlx::query_as::<_, (i64, Option<String>)>(
            "SELECT count(*), max(trace_id) FROM audit_logs
             WHERE action='devrail.run.reconcile_interrupt' AND target_id=$1",
        )
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .expect("interruption audit");
        assert_eq!(interruption_audit.0, 1);
        assert!(interruption_audit
            .1
            .is_some_and(|trace| uuid::Uuid::parse_str(&trace).is_ok()));
        sqlx::query("UPDATE devrail_tasks SET status='running' WHERE id=$1")
            .bind(task_id)
            .execute(&pool)
            .await
            .expect("restore running task for stale reconciliation");

        let first = reconcile_scheduler_state(&pool, &[], 30)
            .await
            .expect("first reconciliation");
        let second = reconcile_scheduler_state(&pool, &[], 30)
            .await
            .expect("second reconciliation");
        assert_eq!(first.stale_runs, 1);
        assert_eq!(second.stale_runs, 0);
        let (status, cleanup) = sqlx::query_as::<_, (String, String)>(
            "SELECT status, cleanup_status FROM devrail_runs WHERE id=$1",
        )
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .expect("reconciled run");
        assert_eq!((status.as_str(), cleanup.as_str()), ("failed", "completed"));
        let notifications = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM devrail_notifications
             WHERE recipient_user_id=$1 AND source_key=$2",
        )
        .bind(owner_user_id)
        .bind(format!("run:{run_id}:supervisor_process_missing"))
        .fetch_one(&pool)
        .await
        .expect("notification count");
        assert_eq!(notifications, 1);
        let stale_audits = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_logs
             WHERE action='devrail.run.reconcile' AND target_id=$1",
        )
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .expect("stale audit count");
        assert_eq!(stale_audits, 1);

        sqlx::query("UPDATE devrail_tasks SET status='running' WHERE id=$1")
            .bind(task_id)
            .execute(&pool)
            .await
            .expect("prepare restart recovery run");
        let restart_key = format!("scheduler:{task_id}:3");
        let mut tx = pool.begin().await.expect("begin restart run");
        let restart_run = devrail_runs::create_run(
            &mut tx,
            &devrail_runs::NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: &restart_key,
                attempt: 3,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: "0000000000000000000000000000000000000000000000000000000000000000",
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: Some(run_id),
                parent_turn_id: Some("turn-parent"),
                branch_name: None,
                branch_expires_at: None,
                cwd: "/tmp",
                policy: &policy,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("create restart run")
        .expect("restart run inserted");
        tx.commit().await.expect("commit restart run");
        let first_restart = devrail_runs::mark_unrecoverable_runs(&pool)
            .await
            .expect("first restart reconciliation");
        let second_restart = devrail_runs::mark_unrecoverable_runs(&pool)
            .await
            .expect("second restart reconciliation");
        assert_eq!(first_restart, 1);
        assert_eq!(second_restart, 0);
        let restart_state = sqlx::query_as::<_, (String, Option<String>, String)>(
            "SELECT status, exit_reason, cleanup_status FROM devrail_runs WHERE id=$1",
        )
        .bind(restart_run.id)
        .fetch_one(&pool)
        .await
        .expect("read restart state");
        assert_eq!(restart_state.0, "failed");
        assert_eq!(restart_state.1.as_deref(), Some("supervisor_restart"));
        assert_eq!(restart_state.2, "completed");
        let restart_notifications = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM devrail_notifications
             WHERE recipient_user_id=$1 AND source_key=$2",
        )
        .bind(owner_user_id)
        .bind(format!("run:{}:supervisor_restart", restart_run.id))
        .fetch_one(&pool)
        .await
        .expect("restart notification count");
        assert_eq!(restart_notifications, 1);
        let restart_audits = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_logs
             WHERE action='devrail.run.reconcile_restart' AND target_id=$1",
        )
        .bind(restart_run.id)
        .fetch_one(&pool)
        .await
        .expect("restart audit count");
        assert_eq!(restart_audits, 1);
    }
}
