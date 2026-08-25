//! Scoped persistence for Harness Supervisor runs and their append-only events.

use crate::access::ActorContext;
use crate::models::{DevRailRunEventRow, DevRailRunRow};
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const RUN_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, branch_name, branch_expires_at, status, thread_id, turn_id, harness_version, model_id, cwd, policy, startup_args_summary, exit_reason, exit_code, stderr_summary, trace_id, recovery_suggestion, recovery_attempts, started_at, completed_at, created_at, updated_at";
const EVENT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, run_id, cursor, event_type, source_event_id, idempotency_key, payload, summary, occurred_at";
const MAX_TRANSPORT_RECOVERY_ATTEMPTS: i32 = 2;

pub(crate) fn can_transport_recover(recovery_attempts: i32) -> bool {
    recovery_attempts < MAX_TRANSPORT_RECOVERY_ATTEMPTS
}

fn scope(alias: &str) -> String {
    format!(
        "{alias}.organization_id = $2 AND ($1 = 'all' OR ($1 = 'organization') OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments)))"
    )
}

pub(crate) struct NewRun<'a> {
    pub actor: &'a ActorContext,
    pub task_id: i64,
    pub snapshot_id: i64,
    pub idempotency_key: &'a str,
    pub branch_name: Option<&'a str>,
    pub branch_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cwd: &'a str,
    pub policy: &'a Value,
    pub startup_args: &'a Value,
    pub model_id: Option<&'a str>,
    pub department_id: Option<i64>,
}

pub(crate) struct TerminalRunUpdate<'a> {
    pub run_id: i64,
    pub status: &'a str,
    pub exit_reason: &'a str,
    pub exit_code: Option<i32>,
    pub stderr_summary: Option<&'a str>,
    pub trace_id: &'a str,
    pub recovery_suggestion: Option<&'a str>,
}

pub(crate) struct NewRunEvent<'a> {
    pub run_id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub event_type: &'a str,
    pub source_event_id: Option<&'a str>,
    pub idempotency_key: &'a str,
    pub payload: &'a Value,
    pub summary: Option<&'a str>,
}

pub(crate) async fn create_snapshot(
    c: &mut PgConnection,
    actor: &ActorContext,
    task_id: i64,
    snapshot: &Value,
    department_id: Option<i64>,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO devrail_task_snapshots (organization_id, department_id, owner_user_id, task_id, snapshot) VALUES ($1,$2,$3,$4,$5) RETURNING id")
        .bind(actor.organization_id).bind(department_id).bind(actor.user_id).bind(task_id).bind(snapshot)
        .fetch_one(c).await
}

pub(crate) async fn create_run(
    c: &mut PgConnection,
    input: &NewRun<'_>,
) -> Result<DevRailRunRow, sqlx::Error> {
    let sql = format!("INSERT INTO devrail_runs (organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, branch_name, branch_expires_at, status, cwd, policy, startup_args_summary, model_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'starting',$9,$10,$11,$12) ON CONFLICT (organization_id, task_id, idempotency_key) DO UPDATE SET updated_at=devrail_runs.updated_at RETURNING {RUN_COLUMNS}");
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(input.actor.organization_id)
        .bind(input.department_id)
        .bind(input.actor.user_id)
        .bind(input.task_id)
        .bind(input.snapshot_id)
        .bind(input.idempotency_key)
        .bind(input.branch_name)
        .bind(input.branch_expires_at)
        .bind(input.cwd)
        .bind(input.policy)
        .bind(input.startup_args)
        .bind(input.model_id)
        .fetch_one(c)
        .await
}

pub(crate) async fn clear_expired_branch(
    c: &mut PgConnection,
    run_id: i64,
) -> Result<Option<(i64, String)>, sqlx::Error> {
    sqlx::query_as("UPDATE devrail_runs SET branch_name=NULL,branch_expires_at=NULL,updated_at=now() WHERE id=$1 AND branch_expires_at<=now() AND branch_name IS NOT NULL RETURNING task_id,branch_name")
        .bind(run_id).fetch_optional(c).await
}

pub(crate) async fn expired_branches(
    pool: &PgPool,
) -> Result<Vec<(i64, i64, String)>, sqlx::Error> {
    sqlx::query_as("SELECT r.id,r.task_id,r.branch_name FROM devrail_runs r WHERE r.branch_expires_at<=now() AND r.branch_name IS NOT NULL ORDER BY r.id LIMIT 50").fetch_all(pool).await
}

pub(crate) async fn update_run_started(
    c: &mut PgConnection,
    run_id: i64,
    thread_id: Option<&str>,
    turn_id: Option<&str>,
    harness_version: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devrail_runs SET status='active', thread_id=COALESCE($2,thread_id), turn_id=COALESCE($3,turn_id), harness_version=COALESCE($4,harness_version), started_at=COALESCE(started_at,now()), updated_at=now() WHERE id=$1")
        .bind(run_id).bind(thread_id).bind(turn_id).bind(harness_version).execute(c).await.map(|_| ())
}

pub(crate) async fn update_run_terminal(
    c: &mut PgConnection,
    input: &TerminalRunUpdate<'_>,
) -> Result<bool, sqlx::Error> {
    sqlx::query("UPDATE devrail_runs SET status=$2, exit_reason=$3, exit_code=$4, stderr_summary=$5, trace_id=$6, recovery_suggestion=$7, completed_at=COALESCE(completed_at,now()), updated_at=now() WHERE id=$1 AND status NOT IN ('completed','failed','cancelled')")
        .bind(input.run_id).bind(input.status).bind(input.exit_reason).bind(input.exit_code).bind(input.stderr_summary).bind(input.trace_id).bind(input.recovery_suggestion).execute(c).await.map(|result| result.rows_affected() == 1)
}

pub(crate) async fn prepare_transport_recovery(
    pool: &PgPool,
    run_id: i64,
    reason: &str,
) -> Result<bool, sqlx::Error> {
    let recovery_limit = if can_transport_recover(0) {
        MAX_TRANSPORT_RECOVERY_ATTEMPTS
    } else {
        0
    };
    Ok(sqlx::query("UPDATE devrail_runs SET status='starting', recovery_attempts=recovery_attempts+1, exit_reason=$2, exit_code=NULL, stderr_summary=NULL, completed_at=NULL, recovery_suggestion='Harness 连接中断；系统正在自动恢复', updated_at=now() WHERE id=$1 AND status IN ('starting','active') AND recovery_attempts < $3")
        .bind(run_id)
        .bind(reason)
        .bind(recovery_limit)
        .execute(pool)
        .await?
        .rows_affected() == 1)
}

pub(crate) async fn update_task_status(
    c: &mut PgConnection,
    task_id: i64,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devrail_tasks SET status=$2, updated_at=now() WHERE id=$1")
        .bind(task_id)
        .bind(status)
        .execute(c)
        .await
        .map(|_| ())
}

pub(crate) async fn mark_quality_gate_failed(
    c: &mut PgConnection,
    run_id: i64,
    task_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devrail_runs SET status='failed',exit_reason='quality_gate_failed',recovery_suggestion='质量门禁未通过；请查看门禁结果后重试',completed_at=COALESCE(completed_at,now()),updated_at=now() WHERE id=$1")
        .bind(run_id)
        .execute(&mut *c)
        .await?;
    update_task_status(c, task_id, "failed").await
}

pub(crate) async fn list_recoverable_runs(
    pool: &PgPool,
) -> Result<Vec<DevRailRunRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(format!("SELECT {RUN_COLUMNS} FROM devrail_runs WHERE status IN ('starting','active') AND thread_id IS NOT NULL ORDER BY id ASC")))
        .fetch_all(pool)
        .await
}

pub(crate) async fn find_for_recovery(
    pool: &PgPool,
    run_id: i64,
) -> Result<Option<DevRailRunRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(format!(
        "SELECT {RUN_COLUMNS} FROM devrail_runs WHERE id=$1"
    )))
    .bind(run_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn mark_unrecoverable_runs(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE devrail_runs SET status='failed', exit_reason='supervisor_restart', recovery_suggestion='服务重启后运行无法自动恢复；请使用相同快照重试', completed_at=COALESCE(completed_at,now()), updated_at=now() WHERE status IN ('starting','active') AND thread_id IS NULL")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn find_run(
    pool: &PgPool,
    actor: &ActorContext,
    run_id: i64,
) -> Result<Option<DevRailRunRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {RUN_COLUMNS} FROM devrail_runs r WHERE r.id=$5 AND {}", scope("r"));
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(run_id)
        .fetch_optional(pool)
        .await
}

pub async fn find_run_by_idempotency(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    idempotency_key: &str,
) -> Result<Option<DevRailRunRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {RUN_COLUMNS} FROM devrail_runs r WHERE r.task_id=$5 AND r.idempotency_key=$6 AND {}", scope("r"));
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
        .bind(idempotency_key)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn task_id_for_run(pool: &PgPool, run_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT task_id FROM devrail_runs WHERE id=$1")
        .bind(run_id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn policy_version_for_run(
    pool: &PgPool,
    run_id: i64,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT policy->>'version' FROM devrail_runs WHERE id=$1")
        .bind(run_id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn has_failed_quality_gate(
    c: &mut PgConnection,
    run_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM devrail_run_events WHERE run_id=$1 AND event_type='quality_gate' AND COALESCE(payload->>'status','') NOT IN ('passed','success','succeeded'))")
        .bind(run_id)
        .fetch_one(c)
        .await
}

pub(crate) async fn find_snapshot(
    pool: &PgPool,
    actor: &ActorContext,
    snapshot_id: i64,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT s.snapshot FROM devrail_task_snapshots s WHERE s.id=$1 AND s.organization_id=$2 AND ($3='all' OR $3='organization' OR ($3='self' AND s.owner_user_id=$4) OR ($3='department' AND s.department_id=$5) OR ($3='department_and_children' AND s.department_id IN (SELECT id FROM departments WHERE organization_id=$2)))")
        .bind(snapshot_id).bind(actor.organization_id).bind(actor.data_scope.as_str()).bind(actor.user_id).bind(actor.department_id).fetch_optional(pool).await
}

pub async fn list_runs(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    page: i64,
    size: i64,
) -> Result<Vec<DevRailRunRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {RUN_COLUMNS} FROM devrail_runs r WHERE r.task_id=$5 AND {} ORDER BY r.created_at DESC, r.id DESC LIMIT $6 OFFSET $7", scope("r"));
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(pool)
        .await
}

pub async fn count_runs(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
) -> Result<i64, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT count(*) FROM devrail_runs r WHERE r.task_id=$5 AND {}", scope("r"));
    sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn append_event(
    c: &mut PgConnection,
    input: &NewRunEvent<'_>,
) -> Result<DevRailRunEventRow, sqlx::Error> {
    sqlx::query("SELECT id FROM devrail_runs WHERE id=$1 FOR UPDATE")
        .bind(input.run_id)
        .execute(&mut *c)
        .await?;
    let sql = format!("INSERT INTO devrail_run_events (organization_id, department_id, owner_user_id, run_id, cursor, event_type, source_event_id, idempotency_key, payload, summary) VALUES ($1,$2,$3,$4,COALESCE((SELECT max(cursor)+1 FROM devrail_run_events WHERE run_id=$4),1),$5,$6,$7,$8,$9) ON CONFLICT (run_id,idempotency_key) DO UPDATE SET idempotency_key=EXCLUDED.idempotency_key RETURNING {EVENT_COLUMNS}");
    sqlx::query_as::<_, DevRailRunEventRow>(AssertSqlSafe(sql))
        .bind(input.organization_id)
        .bind(input.department_id)
        .bind(input.owner_user_id)
        .bind(input.run_id)
        .bind(input.event_type)
        .bind(input.source_event_id)
        .bind(input.idempotency_key)
        .bind(input.payload)
        .bind(input.summary)
        .fetch_one(c)
        .await
}

pub async fn list_events(
    pool: &PgPool,
    actor: &ActorContext,
    run_id: i64,
    after_cursor: i64,
    limit: i64,
) -> Result<Vec<DevRailRunEventRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {EVENT_COLUMNS} FROM devrail_run_events e WHERE e.run_id=$5 AND e.cursor>$6 AND {} ORDER BY e.cursor ASC LIMIT $7", scope("e"));
    sqlx::query_as::<_, DevRailRunEventRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(run_id)
        .bind(after_cursor)
        .bind(limit)
        .fetch_all(pool)
        .await
}

pub async fn find_quality_gate_log(
    pool: &PgPool,
    actor: &ActorContext,
    run_id: i64,
    log_ref: &str,
) -> Result<Option<DevRailRunEventRow>, sqlx::Error> {
    let sql = format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {EVENT_COLUMNS} FROM devrail_run_events e WHERE e.run_id=$5 AND e.event_type='quality_gate' AND e.payload->>'log_ref'=$6 AND {} LIMIT 1", scope("e"));
    sqlx::query_as::<_, DevRailRunEventRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(run_id)
        .bind(log_ref)
        .fetch_optional(pool)
        .await
}
