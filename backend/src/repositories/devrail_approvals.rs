//! Scoped persistence for approval requests and append-only decisions.

use crate::access::ActorContext;
use crate::models::DevRailApprovalRow;
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const APPROVAL_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, run_id, event_id, idempotency_key, tool_name, args_summary, cwd, impact_scope, risk_level, requested_by, decided_by, status, decision_reason, expires_at, policy_version, created_at, updated_at";

fn scope(alias: &str) -> String {
    format!("{alias}.organization_id = $2 AND ($1 = 'all' OR $1 = 'organization' OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments)))")
}

pub(crate) struct NewApproval<'a> {
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub run_id: i64,
    pub event_id: Option<i64>,
    pub idempotency_key: &'a str,
    pub tool_name: &'a str,
    pub args_summary: &'a Value,
    pub cwd: &'a str,
    pub impact_scope: Option<&'a str>,
    pub risk_level: &'a str,
    pub requested_by: i64,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub policy_version: Option<&'a str>,
}

pub(crate) struct ApprovalDecision<'a> {
    pub actor: &'a ActorContext,
    pub id: i64,
    pub decision: &'a str,
    pub reason: Option<&'a str>,
}

pub(crate) async fn create_pending(
    c: &mut PgConnection,
    input: &NewApproval<'_>,
) -> Result<DevRailApprovalRow, sqlx::Error> {
    let sql = format!("INSERT INTO devrail_approvals (organization_id, department_id, owner_user_id, run_id, event_id, idempotency_key, tool_name, args_summary, cwd, impact_scope, risk_level, requested_by, expires_at, policy_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT (run_id,idempotency_key) DO UPDATE SET updated_at=devrail_approvals.updated_at RETURNING {APPROVAL_COLUMNS}");
    sqlx::query_as::<_, DevRailApprovalRow>(AssertSqlSafe(sql))
        .bind(input.organization_id)
        .bind(input.department_id)
        .bind(input.owner_user_id)
        .bind(input.run_id)
        .bind(input.event_id)
        .bind(input.idempotency_key)
        .bind(input.tool_name)
        .bind(input.args_summary)
        .bind(input.cwd)
        .bind(input.impact_scope)
        .bind(input.risk_level)
        .bind(input.requested_by)
        .bind(input.expires_at)
        .bind(input.policy_version)
        .fetch_one(c)
        .await
}

pub async fn find(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailApprovalRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {APPROVAL_COLUMNS} FROM devrail_approvals a WHERE a.id=$5 AND {}", scope("a"));
    sqlx::query_as::<_, DevRailApprovalRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    page: i64,
    size: i64,
) -> Result<Vec<DevRailApprovalRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {APPROVAL_COLUMNS} FROM devrail_approvals a WHERE a.status='pending' AND {} ORDER BY a.expires_at ASC, a.id ASC LIMIT $5 OFFSET $6", scope("a"));
    sqlx::query_as::<_, DevRailApprovalRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(pool)
        .await
}

pub async fn count(pool: &PgPool, actor: &ActorContext) -> Result<i64, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT count(*) FROM devrail_approvals a WHERE a.status='pending' AND {}", scope("a"));
    sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn decide(
    c: &mut PgConnection,
    input: &ApprovalDecision<'_>,
) -> Result<Option<DevRailApprovalRow>, sqlx::Error> {
    let updated = sqlx::query_as::<_, DevRailApprovalRow>(AssertSqlSafe(format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) UPDATE devrail_approvals a SET status=$6, decided_by=$3, decision_reason=$7, updated_at=now() WHERE a.id=$5 AND a.status='pending' AND a.expires_at > now() AND {} RETURNING {APPROVAL_COLUMNS}", scope("a"))))
        .bind(input.actor.data_scope.as_str()).bind(input.actor.organization_id).bind(input.actor.user_id).bind(input.actor.department_id).bind(input.id).bind(input.decision).bind(input.reason).fetch_optional(&mut *c).await?;
    let Some(row) = updated else {
        return Ok(None);
    };
    sqlx::query("INSERT INTO devrail_approval_decisions (organization_id, department_id, owner_user_id, approval_id, decided_by, decision, reason) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(row.organization_id).bind(row.department_id).bind(row.owner_user_id).bind(row.id).bind(input.actor.user_id).bind(input.decision).bind(input.reason).execute(c).await?;
    Ok(Some(row))
}

pub(crate) async fn withdraw(
    c: &mut PgConnection,
    input: &ApprovalDecision<'_>,
) -> Result<Option<DevRailApprovalRow>, sqlx::Error> {
    let updated = sqlx::query_as::<_, DevRailApprovalRow>(AssertSqlSafe(format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) UPDATE devrail_approvals a SET status='cancelled', decided_by=$3, decision_reason=$6, updated_at=now() WHERE a.id=$5 AND a.status='pending' AND a.requested_by=$3 AND {} RETURNING {APPROVAL_COLUMNS}", scope("a"))))
        .bind(input.actor.data_scope.as_str()).bind(input.actor.organization_id).bind(input.actor.user_id).bind(input.actor.department_id).bind(input.id).bind(input.reason).fetch_optional(&mut *c).await?;
    let Some(row) = updated else {
        return Ok(None);
    };
    sqlx::query("INSERT INTO devrail_approval_decisions (organization_id, department_id, owner_user_id, approval_id, decided_by, decision, reason) VALUES ($1,$2,$3,$4,$5,'cancelled',$6)")
        .bind(row.organization_id).bind(row.department_id).bind(row.owner_user_id).bind(row.id).bind(input.actor.user_id).bind(input.reason).execute(c).await?;
    Ok(Some(row))
}

pub(crate) async fn expire_due(pool: &PgPool) -> Result<Vec<DevRailApprovalRow>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let rows = sqlx::query_as::<_, DevRailApprovalRow>(AssertSqlSafe(format!("UPDATE devrail_approvals SET status='expired', decision_reason='审批已过期', updated_at=now() WHERE status='pending' AND expires_at <= now() RETURNING {APPROVAL_COLUMNS}")))
        .fetch_all(&mut *tx).await?;
    for row in &rows {
        sqlx::query("INSERT INTO devrail_approval_decisions (organization_id, department_id, owner_user_id, approval_id, decided_by, decision, reason) VALUES ($1,$2,$3,$4,NULL,'expired',$5)")
            .bind(row.organization_id).bind(row.department_id).bind(row.owner_user_id).bind(row.id).bind("审批已过期").execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(rows)
}

pub(crate) async fn mark_waiting(
    c: &mut PgConnection,
    run_id: i64,
    task_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devrail_runs SET status='awaiting_approval', updated_at=now() WHERE id=$1 AND status IN ('starting','active')").bind(run_id).execute(&mut *c).await?;
    sqlx::query("UPDATE devrail_tasks SET status='awaiting_approval', updated_at=now() WHERE id=$1")
        .bind(task_id)
        .execute(c)
        .await
        .map(|_| ())
}

pub(crate) async fn mark_resumed(
    c: &mut PgConnection,
    run_id: i64,
    task_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devrail_runs SET status='active', updated_at=now() WHERE id=$1 AND status='awaiting_approval'").bind(run_id).execute(&mut *c).await?;
    sqlx::query("UPDATE devrail_tasks SET status='running', updated_at=now() WHERE id=$1 AND status='awaiting_approval'").bind(task_id).execute(c).await.map(|_| ())
}
