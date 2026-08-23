use crate::access::ActorContext;
use crate::models::DevRailProjectMemberRow;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const COLUMNS: &str = "m.id, m.organization_id, m.department_id, m.owner_user_id, m.project_id, m.user_id, u.username, u.display_name, m.role, m.joined_at, m.revoked_at";

fn scope(alias: &str) -> String {
    format!("($1 = 'all' OR {alias}.organization_id = $2 AND ($1 = 'organization' OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM departments WHERE organization_id = $2))))")
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
) -> Result<Vec<DevRailProjectMemberRow>, sqlx::Error> {
    let sql = format!("SELECT {COLUMNS} FROM devrail_project_members m JOIN users u ON u.id = m.user_id WHERE m.project_id=$5 AND m.revoked_at IS NULL AND {} ORDER BY m.role, u.display_name, m.id", scope("m"));
    sqlx::query_as::<_, DevRailProjectMemberRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(project_id)
        .fetch_all(pool)
        .await
}

pub async fn add(
    connection: &mut PgConnection,
    actor: &ActorContext,
    project_id: i64,
    user_id: i64,
    role: &str,
) -> Result<DevRailProjectMemberRow, sqlx::Error> {
    let sql = "INSERT INTO devrail_project_members (organization_id, department_id, owner_user_id, project_id, user_id, role) SELECT p.organization_id, p.department_id, $3, p.id, u.id, $5 FROM devrail_projects p JOIN users u ON u.id=$4 WHERE p.id=$6 AND p.organization_id=$1 ON CONFLICT (project_id,user_id) DO UPDATE SET department_id=EXCLUDED.department_id, owner_user_id=EXCLUDED.owner_user_id, role=EXCLUDED.role, revoked_at=NULL RETURNING id, organization_id, department_id, owner_user_id, project_id, user_id, (SELECT username FROM users WHERE id=devrail_project_members.user_id), (SELECT display_name FROM users WHERE id=devrail_project_members.user_id), role, joined_at, revoked_at";
    sqlx::query_as::<_, DevRailProjectMemberRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(actor.department_id)
        .bind(actor.user_id)
        .bind(user_id)
        .bind(role)
        .bind(project_id)
        .fetch_one(connection)
        .await
}

pub async fn revoke(
    connection: &mut PgConnection,
    actor: &ActorContext,
    project_id: i64,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE devrail_project_members m SET revoked_at=now() WHERE m.project_id=$5 AND m.user_id=$6 AND m.revoked_at IS NULL AND m.organization_id=$2 AND ($1='all' OR ($1='organization') OR ($1='self' AND m.owner_user_id=$3) OR ($1='department' AND m.department_id=$4) OR ($1='department_and_children' AND m.department_id IN (SELECT id FROM departments WHERE organization_id=$2))) AND NOT EXISTS (SELECT 1 FROM devrail_project_members keep WHERE keep.project_id=m.project_id AND keep.user_id=m.user_id AND keep.role='owner' AND keep.revoked_at IS NULL)").bind(actor.data_scope.as_str()).bind(actor.organization_id).bind(actor.user_id).bind(actor.department_id).bind(project_id).bind(user_id).execute(connection).await?;
    Ok(result.rows_affected() > 0)
}
