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
const REPOSITORY_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, name, remote_url, protocol, default_branch, credential_ref, last_sync_status, last_head_sha, created_at, updated_at, archived_at";
const ENVIRONMENT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, name, workspace_root, network_mode, tool_policy, secret_refs, max_duration_secs, enabled, created_at, updated_at, archived_at";
const TASK_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, assignee_user_id, title, goal, background, acceptance_criteria, constraints, priority, status, labels, due_at, created_at, updated_at, archived_at";

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
    project_id: i64,
    id: i64,
    status: &str,
    head_sha: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE devrail_repositories SET last_sync_status=$5,last_head_sha=$6,updated_at=now() WHERE id=$1 AND project_id=$2 AND organization_id=$3 AND ($4='all' OR owner_user_id=$7 OR $4='organization' OR ($4 IN ('department','department_and_children') AND department_id=$8)) AND archived_at IS NULL")
        .bind(id).bind(project_id).bind(actor.organization_id).bind(actor.data_scope.as_str()).bind(status).bind(head_sha).bind(actor.user_id).bind(actor.department_id).execute(c).await?;
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

pub(crate) async fn create_task(
    c: &mut PgConnection,
    actor: &ActorContext,
    n: &NewTask<'_>,
) -> Result<DevRailTaskRow, sqlx::Error> {
    let sql=format!("INSERT INTO devrail_tasks (organization_id,department_id,owner_user_id,project_id,assignee_user_id,title,goal,background,acceptance_criteria,constraints,priority,labels,due_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING {TASK_COLUMNS}");
    sqlx::query_as::<_, DevRailTaskRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(n.department_id)
        .bind(actor.user_id)
        .bind(n.project_id)
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
    let r=sqlx::query("UPDATE devrail_tasks SET title=COALESCE($5,title),goal=COALESCE($6,goal),background=CASE WHEN $7 THEN $8 ELSE background END,acceptance_criteria=CASE WHEN $9 THEN $10 ELSE acceptance_criteria END,constraints=CASE WHEN $11 THEN $12 ELSE constraints END,priority=COALESCE($13,priority),status=COALESCE($14,status),assignee_user_id=CASE WHEN $15 THEN $16 ELSE assignee_user_id END,labels=COALESCE($17,labels),due_at=CASE WHEN $18 THEN $19 ELSE due_at END,archived_at=CASE WHEN $14='archived' THEN COALESCE(archived_at,now()) WHEN $14 IS NOT NULL THEN NULL ELSE archived_at END,updated_at=now() WHERE id=$1 AND project_id=$2 AND organization_id=$3 AND ($4='all' OR owner_user_id=$20 OR $4='organization' OR ($4 IN ('department','department_and_children') AND department_id=$21)) AND archived_at IS NULL").bind(id).bind(project_id).bind(actor.organization_id).bind(actor.data_scope.as_str()).bind(u.title).bind(u.goal).bind(u.background_set).bind(u.background).bind(u.acceptance_set).bind(u.acceptance_criteria).bind(u.constraints_set).bind(u.constraints).bind(u.priority).bind(u.status).bind(u.assignee_set).bind(u.assignee_user_id).bind(u.labels).bind(u.due_at_set).bind(u.due_at).bind(actor.user_id).bind(actor.department_id).execute(c).await?;
    Ok(r.rows_affected() > 0)
}
