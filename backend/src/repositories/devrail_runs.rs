//! Scoped persistence for Harness Supervisor runs and their append-only events.

use crate::access::ActorContext;
use crate::models::{DevRailRunEventRow, DevRailRunRow};
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const RUN_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, status, thread_id, turn_id, harness_version, model_id, cwd, policy, startup_args_summary, exit_reason, exit_code, stderr_summary, trace_id, recovery_suggestion, started_at, completed_at, created_at, updated_at";
const EVENT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, run_id, cursor, event_type, source_event_id, idempotency_key, payload, summary, occurred_at";

fn scope(alias: &str) -> String {
    format!(
        "{alias}.organization_id = $2 AND ($1 = 'all' OR ($1 = 'organization') OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments)))"
    )
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

#[allow(
    clippy::too_many_arguments,
    reason = "Repository input mirrors the scoped run record"
)]
pub(crate) async fn create_run(
    c: &mut PgConnection,
    actor: &ActorContext,
    task_id: i64,
    snapshot_id: i64,
    idempotency_key: &str,
    cwd: &str,
    policy: &Value,
    startup_args: &Value,
    model_id: Option<&str>,
    department_id: Option<i64>,
) -> Result<DevRailRunRow, sqlx::Error> {
    let sql = format!("INSERT INTO devrail_runs (organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, status, cwd, policy, startup_args_summary, model_id) VALUES ($1,$2,$3,$4,$5,$6,'starting',$7,$8,$9,$10) ON CONFLICT (organization_id, task_id, idempotency_key) DO UPDATE SET updated_at=devrail_runs.updated_at RETURNING {RUN_COLUMNS}");
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(department_id)
        .bind(actor.user_id)
        .bind(task_id)
        .bind(snapshot_id)
        .bind(idempotency_key)
        .bind(cwd)
        .bind(policy)
        .bind(startup_args)
        .bind(model_id)
        .fetch_one(c)
        .await
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

#[allow(
    clippy::too_many_arguments,
    reason = "Repository input mirrors terminal run metadata"
)]
pub(crate) async fn update_run_terminal(
    c: &mut PgConnection,
    run_id: i64,
    status: &str,
    exit_reason: &str,
    exit_code: Option<i32>,
    stderr_summary: Option<&str>,
    trace_id: &str,
    recovery_suggestion: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devrail_runs SET status=$2, exit_reason=$3, exit_code=$4, stderr_summary=$5, trace_id=$6, recovery_suggestion=$7, completed_at=COALESCE(completed_at,now()), updated_at=now() WHERE id=$1")
        .bind(run_id).bind(status).bind(exit_reason).bind(exit_code).bind(stderr_summary).bind(trace_id).bind(recovery_suggestion).execute(c).await.map(|_| ())
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

pub(crate) async fn recover_stale_runs(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE devrail_runs SET status='failed', exit_reason='supervisor_restart', recovery_suggestion='服务重启后活动运行已停止；请使用相同快照重试', completed_at=COALESCE(completed_at,now()), updated_at=now() WHERE status IN ('starting','active','awaiting_approval')")
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

#[allow(
    clippy::too_many_arguments,
    reason = "Repository input mirrors the append-only event record"
)]
pub(crate) async fn append_event(
    c: &mut PgConnection,
    run_id: i64,
    organization_id: i64,
    department_id: Option<i64>,
    owner_user_id: i64,
    event_type: &str,
    source_event_id: Option<&str>,
    idempotency_key: &str,
    payload: &Value,
    summary: Option<&str>,
) -> Result<DevRailRunEventRow, sqlx::Error> {
    sqlx::query("SELECT id FROM devrail_runs WHERE id=$1 FOR UPDATE")
        .bind(run_id)
        .execute(&mut *c)
        .await?;
    let sql = format!("INSERT INTO devrail_run_events (organization_id, department_id, owner_user_id, run_id, cursor, event_type, source_event_id, idempotency_key, payload, summary) VALUES ($1,$2,$3,$4,COALESCE((SELECT max(cursor)+1 FROM devrail_run_events WHERE run_id=$4),1),$5,$6,$7,$8,$9) ON CONFLICT (run_id,idempotency_key) DO UPDATE SET idempotency_key=EXCLUDED.idempotency_key RETURNING {EVENT_COLUMNS}");
    sqlx::query_as::<_, DevRailRunEventRow>(AssertSqlSafe(sql))
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(run_id)
        .bind(event_type)
        .bind(source_event_id)
        .bind(idempotency_key)
        .bind(payload)
        .bind(summary)
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
