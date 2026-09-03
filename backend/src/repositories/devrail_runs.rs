//! Scoped persistence for Harness Supervisor runs and their append-only events.

use crate::access::ActorContext;
use crate::models::{DevRailRunEventRow, DevRailRunRow};
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const RUN_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, attempt, task_revision, workflow_source, workflow_version, workflow_digest, workflow_snapshot, actor_type, last_heartbeat_at, last_event_at, retry_reason, parent_run_id, parent_turn_id, run_kind, root_run_id, continuation_sequence, continuation_request_id, repair_request_id, repair_sequence, harness_start_key, harness_start_claim_owner, harness_start_claim_token, harness_start_claim_expires_at, harness_started_token, cleanup_status, branch_name, branch_expires_at, status, thread_id, turn_id, harness_version, model_id, cwd, policy, startup_args_summary, exit_reason, exit_code, stderr_summary, trace_id, recovery_suggestion, recovery_attempts, started_at, completed_at, created_at, updated_at";
const EVENT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, run_id, cursor, event_type, source_event_id, idempotency_key, payload, summary, occurred_at";
const MAX_TRANSPORT_RECOVERY_ATTEMPTS: i32 = 2;
pub(crate) const MAX_HOOK_FAILURES: i32 = 5;

pub(crate) fn can_transport_recover(recovery_attempts: i32) -> bool {
    recovery_attempts < MAX_TRANSPORT_RECOVERY_ATTEMPTS
}

pub(crate) const fn hook_failure_breaker_open(failure_count: i32) -> bool {
    failure_count >= MAX_HOOK_FAILURES
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
    pub attempt: i32,
    pub task_revision: i64,
    pub workflow_source: &'a str,
    pub workflow_version: &'a str,
    pub workflow_digest: &'a str,
    pub workflow_snapshot: &'a Value,
    pub actor_type: &'a str,
    pub parent_run_id: Option<i64>,
    pub parent_turn_id: Option<&'a str>,
    pub branch_name: Option<&'a str>,
    pub branch_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cwd: &'a str,
    pub policy: &'a Value,
    pub startup_args: &'a Value,
    pub model_id: Option<&'a str>,
    pub department_id: Option<i64>,
}

pub struct NewContinuationRun<'a> {
    pub actor: &'a ActorContext,
    pub task_id: i64,
    pub snapshot_id: i64,
    pub idempotency_key: &'a str,
    pub task_revision: i64,
    pub workflow_source: &'a str,
    pub workflow_version: &'a str,
    pub workflow_digest: &'a str,
    pub workflow_snapshot: &'a Value,
    pub parent_run_id: i64,
    pub parent_turn_id: &'a str,
    pub thread_id: &'a str,
    pub continuation_request_id: i64,
    pub continuation_sequence: i16,
    pub harness_start_key: &'a str,
    pub cwd: &'a str,
    pub policy: &'a Value,
    pub startup_args: &'a Value,
    pub model_id: Option<&'a str>,
    pub department_id: Option<i64>,
}

pub struct NewRepairRun<'a> {
    pub actor: &'a ActorContext,
    pub task_id: i64,
    pub snapshot_id: i64,
    pub idempotency_key: &'a str,
    pub task_revision: i64,
    pub workflow_source: &'a str,
    pub workflow_version: &'a str,
    pub workflow_digest: &'a str,
    pub workflow_snapshot: &'a Value,
    pub parent_run_id: i64,
    pub parent_turn_id: Option<&'a str>,
    pub repair_request_id: i64,
    pub repair_sequence: i16,
    pub harness_start_key: &'a str,
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
) -> Result<Option<DevRailRunRow>, sqlx::Error> {
    let sql = format!(
        "INSERT INTO devrail_runs (organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, attempt, task_revision, workflow_source, workflow_version, workflow_digest, workflow_snapshot, actor_type, parent_run_id, parent_turn_id, harness_start_key, run_kind, branch_name, branch_expires_at, status, cwd, policy, startup_args_summary, model_id)
         SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,concat('run-start:', md5(concat_ws(':',$1::text,$4::text,$7::text,$6))),CASE WHEN $14 IS NOT NULL OR ($13='system' AND $7 > 1) THEN 'retry' ELSE 'primary' END,$16,$17,'starting',$18,$19,$20,$21
         FROM devrail_tasks t
         WHERE t.id=$4 AND t.organization_id=$1 AND t.revision=$8
           AND t.workflow_source=$9 AND t.workflow_version=$10 AND t.workflow_digest=$11
           AND t.dispatch_snapshot->'workflow'=$12::jsonb
         ON CONFLICT DO NOTHING RETURNING {RUN_COLUMNS}"
    );
    let inserted = sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(input.actor.organization_id)
        .bind(input.department_id)
        .bind(input.actor.user_id)
        .bind(input.task_id)
        .bind(input.snapshot_id)
        .bind(input.idempotency_key)
        .bind(input.attempt)
        .bind(input.task_revision)
        .bind(input.workflow_source)
        .bind(input.workflow_version)
        .bind(input.workflow_digest)
        .bind(input.workflow_snapshot)
        .bind(input.actor_type)
        .bind(input.parent_run_id)
        .bind(input.parent_turn_id)
        .bind(input.branch_name)
        .bind(input.branch_expires_at)
        .bind(input.cwd)
        .bind(input.policy)
        .bind(input.startup_args)
        .bind(input.model_id)
        .fetch_optional(&mut *c)
        .await?;
    let Some(inserted) = inserted else {
        return Ok(None);
    };
    sqlx::query(
        "UPDATE devrail_runs child
         SET root_run_id=CASE
             WHEN child.parent_run_id IS NULL THEN child.id
             ELSE parent.root_run_id
         END,
         updated_at=now()
         FROM devrail_runs parent
         WHERE child.id=$1 AND child.parent_run_id IS NOT NULL AND parent.id=child.parent_run_id",
    )
    .bind(inserted.id)
    .execute(&mut *c)
    .await?;
    sqlx::query(
        "UPDATE devrail_runs SET root_run_id=id, updated_at=now()
         WHERE id=$1 AND parent_run_id IS NULL AND root_run_id IS NULL",
    )
    .bind(inserted.id)
    .execute(&mut *c)
    .await?;
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(format!(
        "SELECT {RUN_COLUMNS} FROM devrail_runs WHERE id=$1"
    )))
    .bind(inserted.id)
    .fetch_one(&mut *c)
    .await
    .map(Some)
}

pub async fn create_continuation_run(
    c: &mut PgConnection,
    input: &NewContinuationRun<'_>,
) -> Result<Option<DevRailRunRow>, sqlx::Error> {
    let sql = format!(
        "INSERT INTO devrail_runs (organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, attempt, task_revision, workflow_source, workflow_version, workflow_digest, workflow_snapshot, actor_type, parent_run_id, parent_turn_id, run_kind, root_run_id, continuation_sequence, continuation_request_id, harness_start_key, status, thread_id, cwd, policy, startup_args_summary, model_id) SELECT $1,$2,$3,$4,$5,$6,COALESCE((SELECT MAX(attempt)+1 FROM devrail_runs WHERE task_id=$4),1),$7,$8,$9,$10,$11,'system',$12,$13,'continuation',COALESCE(source.root_run_id,source.id),$14,$15,$16,'starting',$17,$18,$19,$20,$21 FROM devrail_runs source JOIN devrail_tasks task ON task.id=source.task_id AND task.organization_id=source.organization_id WHERE source.id=$12 AND source.organization_id=$1 AND source.task_id=$4 AND source.status IN ('completed','failed','cancelled') AND task.revision=$7 AND task.status='continuation_pending' AND NOT EXISTS (SELECT 1 FROM devrail_runs active WHERE active.task_id=$4 AND active.organization_id=$1 AND active.status IN ('starting','active','awaiting_approval')) ON CONFLICT DO NOTHING RETURNING {RUN_COLUMNS}"
    );
    let inserted = sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(input.actor.organization_id)
        .bind(input.department_id)
        .bind(input.actor.user_id)
        .bind(input.task_id)
        .bind(input.snapshot_id)
        .bind(input.idempotency_key)
        .bind(input.task_revision)
        .bind(input.workflow_source)
        .bind(input.workflow_version)
        .bind(input.workflow_digest)
        .bind(input.workflow_snapshot)
        .bind(input.parent_run_id)
        .bind(input.parent_turn_id)
        .bind(input.continuation_sequence)
        .bind(input.continuation_request_id)
        .bind(input.harness_start_key)
        .bind(input.thread_id)
        .bind(input.cwd)
        .bind(input.policy)
        .bind(input.startup_args)
        .bind(input.model_id)
        .fetch_optional(&mut *c)
        .await?;
    if inserted.is_some() {
        return Ok(inserted);
    }
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(format!(
        "SELECT {RUN_COLUMNS} FROM devrail_runs
         WHERE organization_id=$1 AND continuation_request_id=$2
         FOR UPDATE"
    )))
    .bind(input.actor.organization_id)
    .bind(input.continuation_request_id)
    .fetch_optional(&mut *c)
    .await
}

pub async fn create_repair_run(
    c: &mut PgConnection,
    input: &NewRepairRun<'_>,
) -> Result<Option<DevRailRunRow>, sqlx::Error> {
    let sql = format!(
        "INSERT INTO devrail_runs (organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, attempt, task_revision, workflow_source, workflow_version, workflow_digest, workflow_snapshot, actor_type, parent_run_id, parent_turn_id, run_kind, root_run_id, repair_request_id, repair_sequence, harness_start_key, status, cwd, policy, startup_args_summary, model_id)
         SELECT $1,$2,$3,$4,$5,$6,
                COALESCE((SELECT MAX(attempt)+1 FROM devrail_runs WHERE task_id=$4 AND organization_id=$1),1),
                $7,$8,$9,$10,$11,'system',$12,$13,'repair',
                COALESCE(source.root_run_id,source.id),$14,$15,$16,'starting',$17,$18,$19,$20
         FROM devrail_runs source
         JOIN devrail_tasks task ON task.id=source.task_id AND task.organization_id=source.organization_id
         JOIN devrail_repair_requests request
           ON request.id=$14 AND request.organization_id=$1
          AND request.task_id=task.id AND request.source_run_id=source.id
          AND request.repair_sequence=$15
         WHERE source.id=$12 AND source.organization_id=$1 AND source.task_id=$4
           AND source.status='failed' AND task.revision=$7
           AND task.status IN ('repair_pending','repair_running')
           AND request.status IN ('claimed','dispatched','running')
           AND NOT EXISTS (
             SELECT 1 FROM devrail_runs active
             WHERE active.task_id=$4 AND active.organization_id=$1
               AND active.status IN ('starting','active','awaiting_approval')
           )
         ON CONFLICT DO NOTHING RETURNING {RUN_COLUMNS}"
    );
    let inserted = sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(input.actor.organization_id)
        .bind(input.department_id)
        .bind(input.actor.user_id)
        .bind(input.task_id)
        .bind(input.snapshot_id)
        .bind(input.idempotency_key)
        .bind(input.task_revision)
        .bind(input.workflow_source)
        .bind(input.workflow_version)
        .bind(input.workflow_digest)
        .bind(input.workflow_snapshot)
        .bind(input.parent_run_id)
        .bind(input.parent_turn_id)
        .bind(input.repair_request_id)
        .bind(input.repair_sequence)
        .bind(input.harness_start_key)
        .bind(input.cwd)
        .bind(input.policy)
        .bind(input.startup_args)
        .bind(input.model_id)
        .fetch_optional(&mut *c)
        .await?;
    if inserted.is_some() {
        return Ok(inserted);
    }
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(format!(
        "SELECT {RUN_COLUMNS} FROM devrail_runs WHERE organization_id=$1 AND repair_request_id=$2 FOR UPDATE"
    )))
    .bind(input.actor.organization_id)
    .bind(input.repair_request_id)
    .fetch_optional(&mut *c)
    .await
}

pub(crate) async fn next_attempt(c: &mut PgConnection, task_id: i64) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(MAX(attempt), 0) + 1 FROM devrail_runs WHERE task_id = $1")
        .bind(task_id)
        .fetch_one(c)
        .await
}

pub(crate) async fn workflow_snapshot_for_run(
    pool: &PgPool,
    run_id: i64,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT workflow_snapshot FROM devrail_runs WHERE id=$1")
        .bind(run_id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn clear_expired_branch(
    c: &mut PgConnection,
    run_id: i64,
) -> Result<Option<(i64, String)>, sqlx::Error> {
    sqlx::query_as("UPDATE devrail_runs SET branch_name=NULL,branch_expires_at=NULL,updated_at=now() WHERE id=$1 AND branch_expires_at<=now() AND branch_name IS NOT NULL RETURNING task_id,branch_name")
        .bind(run_id).fetch_optional(c).await
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ExpiredBranch {
    pub run_id: i64,
    pub branch_name: String,
    pub remote_url: Option<String>,
    pub credential_ref: Option<String>,
}

pub(crate) async fn expired_branches(pool: &PgPool) -> Result<Vec<ExpiredBranch>, sqlx::Error> {
    sqlx::query_as("SELECT r.id AS run_id,r.branch_name,repo.remote_url,repo.credential_ref FROM devrail_runs r JOIN devrail_tasks t ON t.id=r.task_id LEFT JOIN devrail_repositories repo ON repo.id=t.repository_id WHERE r.branch_expires_at<=now() AND r.branch_name IS NOT NULL ORDER BY r.id LIMIT 50")
        .fetch_all(pool)
        .await
}

pub(crate) async fn update_run_started(
    c: &mut PgConnection,
    run_id: i64,
    thread_id: Option<&str>,
    turn_id: Option<&str>,
    harness_version: Option<&str>,
    start_claim_token: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE devrail_runs SET status='active', thread_id=COALESCE($2,thread_id), turn_id=COALESCE($3,turn_id), harness_version=COALESCE($4,harness_version), harness_started_token=$5, harness_start_claim_owner=NULL, harness_start_claim_token=NULL, harness_start_claim_expires_at=NULL, started_at=COALESCE(started_at,now()), last_heartbeat_at=now(), updated_at=now() WHERE id=$1 AND status IN ('created','starting','active') AND harness_start_key IS NOT NULL AND (harness_start_claim_token=$5 OR (status='active' AND harness_started_token=$5))")
        .bind(run_id).bind(thread_id).bind(turn_id).bind(harness_version).bind(start_claim_token).execute(c).await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn claim_harness_start(
    pool: &PgPool,
    run_id: i64,
    owner: &str,
    token: uuid::Uuid,
    lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_runs
         SET harness_start_claim_owner=CASE WHEN harness_start_key IS NULL THEN harness_start_claim_owner ELSE $2 END,
             harness_start_claim_token=CASE WHEN harness_start_key IS NULL THEN harness_start_claim_token ELSE $3 END,
             harness_start_claim_expires_at=CASE WHEN harness_start_key IS NULL THEN harness_start_claim_expires_at ELSE now()+make_interval(secs => $4) END,
             harness_started_token=CASE WHEN harness_start_key IS NULL THEN harness_started_token ELSE NULL END,
             updated_at=now()
         WHERE id=$1
           AND harness_start_key IS NOT NULL
           AND (
               status='starting'
               AND (harness_start_claim_token IS NULL OR harness_start_claim_expires_at<=now())
           )",
    )
    .bind(run_id)
    .bind(owner)
    .bind(token)
    .bind(lease_seconds.clamp(30, 300))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn release_harness_start(
    pool: &PgPool,
    run_id: i64,
    token: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_runs
         SET harness_start_claim_owner=NULL, harness_start_claim_token=NULL,
             harness_start_claim_expires_at=NULL, updated_at=now()
         WHERE id=$1 AND status='starting' AND harness_start_claim_token=$2",
    )
    .bind(run_id)
    .bind(token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn claim_quality_gate_execution(
    pool: &PgPool,
    run_id: i64,
    owner: &str,
    token: uuid::Uuid,
    lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_runs
         SET quality_gate_claim_owner=$2,
             quality_gate_claim_token=$3,
             quality_gate_claim_expires_at=now()+make_interval(secs => $4),
             updated_at=now()
         WHERE id=$1
           AND status IN ('completed','failed')
           AND (quality_gate_claim_token IS NULL
                OR quality_gate_claim_expires_at IS NULL
                OR quality_gate_claim_expires_at<=now())",
    )
    .bind(run_id)
    .bind(owner)
    .bind(token)
    .bind(lease_seconds.clamp(60, 10_800))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn release_quality_gate_execution(
    pool: &PgPool,
    run_id: i64,
    token: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_runs
         SET quality_gate_claim_owner=NULL,
             quality_gate_claim_token=NULL,
             quality_gate_claim_expires_at=NULL,
             updated_at=now()
         WHERE id=$1 AND quality_gate_claim_token=$2",
    )
    .bind(run_id)
    .bind(token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn update_run_heartbeat(pool: &PgPool, run_id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_runs
         SET last_heartbeat_at = now(), updated_at = now()
         WHERE id = $1 AND status IN ('starting', 'active')",
    )
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn update_run_terminal(
    c: &mut PgConnection,
    input: &TerminalRunUpdate<'_>,
) -> Result<bool, sqlx::Error> {
    sqlx::query("UPDATE devrail_runs SET status=$2, exit_reason=$3, retry_reason=CASE WHEN $2='failed' THEN $3 ELSE retry_reason END, exit_code=$4, stderr_summary=$5, trace_id=$6, recovery_suggestion=$7, completed_at=COALESCE(completed_at,now()), last_heartbeat_at=now(), cleanup_status=CASE WHEN $2 IN ('completed','failed','cancelled') THEN 'completed' ELSE cleanup_status END, updated_at=now() WHERE id=$1 AND status NOT IN ('completed','failed','cancelled')")
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
    sqlx::query("UPDATE devrail_tasks SET status=$2, scheduler_claim_token=NULL, scheduler_claimed_at=NULL, scheduler_retry_at=CASE WHEN $2='queued' THEN scheduler_retry_at ELSE NULL END, scheduler_last_error=CASE WHEN $2='queued' THEN scheduler_last_error ELSE NULL END, updated_at=now() WHERE id=$1")
        .bind(task_id)
        .bind(status)
        .execute(c)
        .await
        .map(|_| ())
}

pub(crate) async fn set_cwd(
    connection: &mut PgConnection,
    run_id: i64,
    cwd: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE devrail_runs SET cwd=$2, updated_at=now() WHERE id=$1")
        .bind(run_id)
        .bind(cwd)
        .execute(connection)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub(crate) async fn mark_quality_gate_failed(
    c: &mut PgConnection,
    run_id: i64,
    task_id: i64,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query("UPDATE devrail_runs SET status='failed',exit_reason='quality_gate_failed',recovery_suggestion='质量门禁未通过；请查看门禁结果后重试',completed_at=COALESCE(completed_at,now()),updated_at=now() WHERE id=$1 AND status='completed'")
        .bind(run_id)
        .execute(&mut *c)
        .await?;
    if updated.rows_affected() != 1 {
        return Ok(false);
    }
    sqlx::query("UPDATE devrail_tasks SET status='failed', scheduler_claim_token=NULL, scheduler_claimed_at=NULL, scheduler_retry_at=NULL, updated_at=now() WHERE id=$1 AND status='succeeded'")
        .bind(task_id)
        .execute(&mut *c)
        .await?;
    Ok(true)
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

pub(crate) async fn claim_approval_recovery(
    c: &mut PgConnection,
    run_id: i64,
    task_id: i64,
) -> Result<bool, sqlx::Error> {
    let Some((run_status, task_status)) = sqlx::query_as::<_, (String, String)>(
        "SELECT r.status, t.status
         FROM devrail_tasks t
         JOIN devrail_runs r ON r.task_id=t.id
         WHERE t.id=$2 AND r.id=$1 AND r.thread_id IS NOT NULL
         FOR UPDATE OF t, r",
    )
    .bind(run_id)
    .bind(task_id)
    .fetch_optional(&mut *c)
    .await?
    else {
        return Ok(false);
    };
    if run_status != "awaiting_approval" || task_status != "awaiting_approval" {
        return Ok(false);
    }
    sqlx::query("UPDATE devrail_runs SET status='starting', updated_at=now() WHERE id=$1")
        .bind(run_id)
        .execute(&mut *c)
        .await?;
    sqlx::query(
        "UPDATE devrail_tasks
         SET status='running', scheduler_claim_token=NULL, scheduler_claimed_at=NULL,
             scheduler_retry_at=NULL, updated_at=now()
         WHERE id=$1",
    )
    .bind(task_id)
    .execute(&mut *c)
    .await?;
    Ok(true)
}

pub(crate) async fn scheduler_retry_policy(
    pool: &PgPool,
    task_id: i64,
) -> Result<(i32, i32), sqlx::Error> {
    sqlx::query_as(
        "SELECT scheduler_attempt, scheduler_max_attempts
         FROM devrail_tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn requeue_task_after_run(
    c: &mut PgConnection,
    task_id: i64,
    retry_at: chrono::DateTime<chrono::Utc>,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE devrail_tasks
         SET status='queued', scheduler_claim_token=NULL, scheduler_claimed_at=NULL,
             scheduler_retry_count=scheduler_retry_count+1,
             scheduler_retry_at=$2, scheduler_last_error=$3, updated_at=now()
         WHERE id=$1",
    )
    .bind(task_id)
    .bind(retry_at)
    .bind(reason.chars().take(500).collect::<String>())
    .execute(c)
    .await?;
    Ok(())
}

pub(crate) async fn record_hook_failure(
    c: &mut PgConnection,
    task_id: i64,
    fingerprint: &str,
) -> Result<i32, sqlx::Error> {
    let count = sqlx::query_scalar::<_, i32>(
        "UPDATE devrail_tasks
         SET hook_failure_count = CASE WHEN hook_failure_fingerprint = $2
             THEN hook_failure_count + 1 ELSE 1 END,
             hook_failure_fingerprint = $2, updated_at = now()
         WHERE id = $1
         RETURNING hook_failure_count",
    )
    .bind(task_id)
    .bind(fingerprint)
    .fetch_one(c)
    .await?;
    Ok(count)
}

pub(crate) async fn clear_hook_failure(
    c: &mut PgConnection,
    task_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE devrail_tasks SET hook_failure_fingerprint=NULL, hook_failure_count=0, updated_at=now() WHERE id=$1",
    )
    .bind(task_id)
    .execute(c)
    .await?;
    Ok(())
}

pub(crate) async fn requeue_task_after_hook_failure(
    c: &mut PgConnection,
    task_id: i64,
    retry_at: chrono::DateTime<chrono::Utc>,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE devrail_tasks
         SET status='queued', scheduler_claim_token=NULL, scheduler_claimed_at=NULL,
             scheduler_retry_count=scheduler_retry_count+1,
             scheduler_retry_at=$2, scheduler_last_error=$3, updated_at=now()
         WHERE id=$1",
    )
    .bind(task_id)
    .bind(retry_at)
    .bind(reason.chars().take(500).collect::<String>())
    .execute(c)
    .await?;
    Ok(())
}

pub(crate) async fn mark_unrecoverable_runs(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let rows = sqlx::query_as::<_, (i64, i64, Option<i64>, i64, String)>("UPDATE devrail_runs SET status='failed', exit_reason='supervisor_restart', retry_reason='服务重启后缺少可恢复 thread', recovery_suggestion='服务重启后运行无法自动恢复；请使用相同快照重试', completed_at=COALESCE(completed_at,now()), cleanup_status='completed', updated_at=now() WHERE status IN ('starting','active') AND thread_id IS NULL RETURNING id, organization_id, department_id, owner_user_id, run_kind")
        .fetch_all(&mut *tx)
        .await?;
    if !rows.is_empty() {
        let run_ids = rows.iter().map(|row| row.0).collect::<Vec<_>>();
        for (run_id, organization_id, department_id, owner_user_id, run_kind) in &rows {
            if run_kind != "continuation" {
                continue;
            }
            let actor = ActorContext {
                actor_type: crate::access::ActorType::System,
                user_id: *owner_user_id,
                session_id: 0,
                organization_id: *organization_id,
                department_id: *department_id,
                data_scope: crate::access::DataScope::All,
                permission_codes: std::collections::BTreeSet::new(),
            };
            crate::repositories::devrail_continuations::complete_for_child_run(
                &mut tx,
                &actor,
                *run_id,
                "supervisor_restart",
                "failed",
            )
            .await?;
        }
        sqlx::query("UPDATE devrail_tasks t SET status='failed', scheduler_claim_token=NULL, scheduler_claimed_at=NULL, updated_at=now() WHERE t.status='running' AND EXISTS (SELECT 1 FROM devrail_runs r WHERE r.task_id=t.id AND r.status='failed' AND r.exit_reason='supervisor_restart' AND r.run_kind <> 'continuation')")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO devrail_notifications
                 (organization_id, department_id, recipient_user_id, event_type,
                  level, title, summary, resource_type, resource_id, deep_link, source_key)
             SELECT r.organization_id, r.department_id, r.owner_user_id,
                    'run.failed', 'error', '运行恢复失败',
                    '服务重启后缺少可恢复 thread，请检查日志后重试',
                    'devrail_run', r.id, '/devrail/runs/' || r.id,
                    'run:' || r.id || ':supervisor_restart'
             FROM devrail_runs r
             WHERE r.status='failed' AND r.exit_reason='supervisor_restart'
             ON CONFLICT (recipient_user_id, source_key) DO NOTHING",
        )
        .execute(&mut *tx)
        .await?;
        let trace = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audit_logs
                 (actor_user_id, action, target_type, target_id, details, trace_id,
                  organization_id, department_id)
             SELECT NULL, 'devrail.run.reconcile_restart', 'devrail_run', r.id,
                    jsonb_build_object(
                        'actorType', 'system',
                        'reason', 'supervisor_restart',
                        'policyVersion', 'devrail-policy-v1'
                    ), $2, r.organization_id, r.department_id
             FROM devrail_runs r WHERE r.id=ANY($1::bigint[])",
        )
        .bind(&run_ids)
        .bind(trace)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO devrail_outbox_events
                 (organization_id, event_type, aggregate_type, aggregate_id, payload)
             SELECT r.organization_id, 'notification.created', 'devrail_run', r.id,
                    jsonb_build_object(
                        'notificationSource', 'run:' || r.id || ':supervisor_restart'
                    )
             FROM devrail_runs r
             WHERE r.status='failed' AND r.exit_reason='supervisor_restart'
             ON CONFLICT DO NOTHING",
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(rows.len() as u64)
}

#[cfg(test)]
pub(crate) async fn create_harness_test_task(
    pool: &PgPool,
    suffix: &str,
) -> Result<(i64, i64, Option<i64>, i64), sqlx::Error> {
    let (owner_user_id, organization_id, department_id) =
        sqlx::query_as::<_, (i64, i64, Option<i64>)>(
            "SELECT id, organization_id, department_id FROM users ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await?;
    let project_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO devrail_projects
             (organization_id, department_id, owner_user_id, slug, name)
         VALUES ($1,$2,$3,$4,'Harness 故障测试') RETURNING id",
    )
    .bind(organization_id)
    .bind(department_id)
    .bind(owner_user_id)
    .bind(format!("harness-fault-{suffix}"))
    .fetch_one(pool)
    .await?;
    let task_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO devrail_tasks
             (organization_id, department_id, owner_user_id, project_id,
              title, goal, status, scheduler_attempt, scheduler_max_attempts)
         VALUES ($1,$2,$3,$4,'Harness 故障测试','验证恢复闭环','running',1,3)
         RETURNING id",
    )
    .bind(organization_id)
    .bind(department_id)
    .bind(owner_user_id)
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    Ok((owner_user_id, organization_id, department_id, task_id))
}

#[cfg(test)]
pub(crate) async fn prepare_harness_test_attempt(
    pool: &PgPool,
    task_id: i64,
    attempt: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE devrail_tasks
         SET status='running', scheduler_attempt=$2, scheduler_retry_at=NULL
         WHERE id=$1",
    )
    .bind(task_id)
    .bind(attempt)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
pub(crate) struct WorkflowE2eFixture {
    pub actor: ActorContext,
    pub project_id: i64,
    pub environment_id: i64,
    pub task_id: i64,
}

#[cfg(test)]
pub(crate) async fn create_workflow_e2e_fixture(
    pool: &PgPool,
    workspace_root: &str,
) -> Result<WorkflowE2eFixture, sqlx::Error> {
    use crate::access::{ActorType, DataScope};
    use std::collections::BTreeSet;

    let (owner_user_id, organization_id, department_id) =
        sqlx::query_as::<_, (i64, i64, Option<i64>)>(
            "SELECT id, organization_id, department_id FROM users ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let project_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO devrail_projects
             (organization_id, department_id, owner_user_id, slug, name)
         VALUES ($1,$2,$3,$4,'Workflow 端到端测试') RETURNING id",
    )
    .bind(organization_id)
    .bind(department_id)
    .bind(owner_user_id)
    .bind(format!("workflow-e2e-{suffix}"))
    .fetch_one(pool)
    .await?;
    let environment_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO devrail_environments
             (organization_id, department_id, owner_user_id, project_id,
              name, workspace_root, network_mode, tool_policy)
         VALUES ($1,$2,$3,$4,$5,$6,'off','{}') RETURNING id",
    )
    .bind(organization_id)
    .bind(department_id)
    .bind(owner_user_id)
    .bind(project_id)
    .bind(format!("workflow-e2e-{suffix}"))
    .bind(workspace_root)
    .fetch_one(pool)
    .await?;
    let task_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO devrail_tasks
             (organization_id, department_id, owner_user_id, project_id,
              environment_id, title, goal, acceptance_criteria)
         VALUES ($1,$2,$3,$4,$5,'工作流端到端任务','执行不可变工作流快照','输入必须来自已渲染工作流')
         RETURNING id",
    )
    .bind(organization_id)
    .bind(department_id)
    .bind(owner_user_id)
    .bind(project_id)
    .bind(environment_id)
    .fetch_one(pool)
    .await?;
    Ok(WorkflowE2eFixture {
        actor: ActorContext {
            actor_type: ActorType::System,
            user_id: owner_user_id,
            session_id: 0,
            organization_id,
            department_id,
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::from([
                "devrail:task:read".to_string(),
                "devrail:task:update".to_string(),
                "devrail:run:execute".to_string(),
            ]),
        },
        project_id,
        environment_id,
        task_id,
    })
}

#[cfg(test)]
pub(crate) async fn count_task_runs_for_test(
    pool: &PgPool,
    task_id: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM devrail_runs WHERE task_id=$1")
        .bind(task_id)
        .fetch_one(pool)
        .await
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

pub(crate) async fn find_retry_parent(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
) -> Result<Option<DevRailRunRow>, sqlx::Error> {
    let sql = format!(
        "WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$4 AND organization_id=$2 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$2) SELECT {RUN_COLUMNS} FROM devrail_runs r WHERE r.task_id=$5 AND r.status='failed' AND {} ORDER BY r.attempt DESC,r.id DESC LIMIT 1",
        scope("r")
    );
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
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
    sqlx::query("UPDATE devrail_runs SET last_event_at=now(), last_heartbeat_at=now(), updated_at=now() WHERE id=$1 AND status IN ('starting','active','awaiting_approval')")
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

pub(crate) async fn append_idempotent_callback_event(
    c: &mut PgConnection,
    input: &NewRunEvent<'_>,
) -> Result<(DevRailRunEventRow, bool), sqlx::Error> {
    let Some(source_event_id) = input.source_event_id else {
        return Err(sqlx::Error::Protocol(
            "repair 回调事件缺少来源事件 ID".to_string(),
        ));
    };
    sqlx::query("SELECT id FROM devrail_runs WHERE id=$1 FOR UPDATE")
        .bind(input.run_id)
        .execute(&mut *c)
        .await?;
    sqlx::query("UPDATE devrail_runs SET last_event_at=now(), last_heartbeat_at=now(), updated_at=now() WHERE id=$1 AND status IN ('starting','active','awaiting_approval')")
        .bind(input.run_id)
        .execute(&mut *c)
        .await?;
    let insert_sql = format!(
        "INSERT INTO devrail_run_events (organization_id, department_id, owner_user_id, run_id, cursor, event_type, source_event_id, idempotency_key, payload, summary) VALUES ($1,$2,$3,$4,COALESCE((SELECT max(cursor)+1 FROM devrail_run_events WHERE run_id=$4),1),$5,$6,$7,$8,$9) ON CONFLICT DO NOTHING RETURNING {EVENT_COLUMNS}"
    );
    if let Some(event) = sqlx::query_as::<_, DevRailRunEventRow>(AssertSqlSafe(insert_sql))
        .bind(input.organization_id)
        .bind(input.department_id)
        .bind(input.owner_user_id)
        .bind(input.run_id)
        .bind(input.event_type)
        .bind(Some(source_event_id))
        .bind(input.idempotency_key)
        .bind(input.payload)
        .bind(input.summary)
        .fetch_optional(&mut *c)
        .await?
    {
        return Ok((event, true));
    }
    let existing = sqlx::query_as::<_, DevRailRunEventRow>(AssertSqlSafe(format!(
        "SELECT {EVENT_COLUMNS} FROM devrail_run_events WHERE (organization_id=$1 AND source_event_id=$2 AND event_type=$3) OR (run_id=$4 AND idempotency_key=$5) ORDER BY CASE WHEN source_event_id=$2 THEN 0 ELSE 1 END, id LIMIT 1 FOR UPDATE"
    )))
    .bind(input.organization_id)
    .bind(source_event_id)
    .bind(input.event_type)
    .bind(input.run_id)
    .bind(input.idempotency_key)
    .fetch_optional(&mut *c)
    .await?
    .ok_or_else(|| sqlx::Error::Protocol("repair 回调事件冲突但原事件不可读".to_string()))?;
    if existing.organization_id != input.organization_id
        || existing.run_id != input.run_id
        || existing.event_type != input.event_type
        || existing.source_event_id.as_deref() != Some(source_event_id)
        || existing.idempotency_key != input.idempotency_key
        || existing.payload != *input.payload
        || existing.summary.as_deref() != input.summary
    {
        return Err(sqlx::Error::Protocol(
            "repair 回调事件 payload 或来源发生漂移".to_string(),
        ));
    }
    Ok((existing, false))
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

#[cfg(test)]
mod workflow_identity_tests {
    use super::*;
    use crate::access::{ActorType, DataScope};
    use serde_json::json;
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn run_insert_requires_and_copies_exact_task_workflow_identity() {
        let Ok(fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = fixture.pool().clone();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let (owner_user_id, organization_id, department_id, task_id) =
            create_harness_test_task(&pool, &suffix)
                .await
                .expect("create task");
        let actor = ActorContext {
            actor_type: ActorType::System,
            user_id: owner_user_id,
            session_id: 0,
            organization_id,
            department_id,
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        };
        let dispatch_snapshot = sqlx::query_scalar::<_, Value>(
            "SELECT dispatch_snapshot FROM devrail_tasks
             WHERE organization_id=$1 AND id=$2",
        )
        .bind(organization_id)
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("read task snapshot");
        let workflow_snapshot = dispatch_snapshot
            .get("workflow")
            .cloned()
            .expect("workflow snapshot");
        let legacy_digest = "0".repeat(64);
        let policy = json!({"version":"workflow-identity-test"});
        let startup_args = json!(["app-server"]);
        let mut tx = pool.begin().await.expect("begin run transaction");
        let snapshot_id =
            create_snapshot(&mut tx, &actor, task_id, &dispatch_snapshot, department_id)
                .await
                .expect("create task snapshot");

        let mismatched = create_run(
            &mut tx,
            &NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: "workflow-mismatch",
                attempt: 1,
                task_revision: 1,
                workflow_source: "repository",
                workflow_version: "legacy-v1",
                workflow_digest: &legacy_digest,
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: None,
                parent_turn_id: None,
                branch_name: None,
                branch_expires_at: None,
                cwd: "/tmp/devrail-workflow-identity",
                policy: &policy,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("reject mismatched identity safely");
        assert!(mismatched.is_none());

        let inserted = create_run(
            &mut tx,
            &NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: "workflow-match",
                attempt: 1,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: &legacy_digest,
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: None,
                parent_turn_id: None,
                branch_name: None,
                branch_expires_at: None,
                cwd: "/tmp/devrail-workflow-identity",
                policy: &policy,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("insert matching identity")
        .expect("matching run created");
        assert_eq!(inserted.workflow_source, "legacy");
        assert_eq!(inserted.workflow_version, "legacy-v1");
        assert_eq!(inserted.workflow_digest, "0".repeat(64));
        assert_eq!(inserted.workflow_snapshot, workflow_snapshot);
        tx.rollback().await.expect("rollback isolated fixture");
        drop(pool);
        fixture.cleanup().await.expect("cleanup run schema");
    }

    #[tokio::test]
    async fn harness_start_claim_accepts_only_the_current_process_token() {
        let Ok(fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = fixture.pool().clone();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let (owner_user_id, organization_id, department_id, task_id) =
            create_harness_test_task(&pool, &suffix)
                .await
                .expect("create task");
        let actor = ActorContext {
            actor_type: ActorType::System,
            user_id: owner_user_id,
            session_id: 0,
            organization_id,
            department_id,
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        };
        let dispatch_snapshot = sqlx::query_scalar::<_, Value>(
            "SELECT dispatch_snapshot FROM devrail_tasks WHERE id=$1",
        )
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("read task snapshot");
        let workflow_snapshot = dispatch_snapshot
            .get("workflow")
            .cloned()
            .expect("workflow snapshot");
        let workflow_digest = "0".repeat(64);
        let mut tx = pool.begin().await.expect("begin run transaction");
        let snapshot_id =
            create_snapshot(&mut tx, &actor, task_id, &dispatch_snapshot, department_id)
                .await
                .expect("create snapshot");
        let run = create_run(
            &mut tx,
            &NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: "start-claim-token",
                attempt: 1,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: &workflow_digest,
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: None,
                parent_turn_id: None,
                branch_name: None,
                branch_expires_at: None,
                cwd: "/tmp/devrail-start-claim",
                policy: &serde_json::json!({}),
                startup_args: &serde_json::json!(["app-server"]),
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("create run")
        .expect("run inserted");
        sqlx::query("UPDATE devrail_runs SET harness_start_key='start-claim-token' WHERE id=$1")
            .bind(run.id)
            .execute(&mut *tx)
            .await
            .expect("set stable start key");
        tx.commit().await.expect("commit run");

        let first_token = uuid::Uuid::new_v4();
        assert!(
            claim_harness_start(&pool, run.id, "supervisor:first", first_token, 120,)
                .await
                .expect("claim first start")
        );
        let mut started_tx = pool.begin().await.expect("begin first start");
        assert!(update_run_started(
            &mut started_tx,
            run.id,
            Some("thread-first"),
            Some("turn-first"),
            None,
            first_token,
        )
        .await
        .expect("persist first start"));
        started_tx.commit().await.expect("commit first start");

        let stale_token = uuid::Uuid::new_v4();
        let mut stale_tx = pool.begin().await.expect("begin stale update");
        assert!(!update_run_started(
            &mut stale_tx,
            run.id,
            Some("thread-stale"),
            Some("turn-stale"),
            None,
            stale_token,
        )
        .await
        .expect("stale update result"));
        stale_tx.commit().await.expect("commit stale update");
        let stored = sqlx::query_as::<_, (String, String)>(
            "SELECT thread_id,turn_id FROM devrail_runs WHERE id=$1",
        )
        .bind(run.id)
        .fetch_one(&pool)
        .await
        .expect("read started run");
        assert_eq!(
            stored,
            ("thread-first".to_string(), "turn-first".to_string())
        );

        assert!(
            prepare_transport_recovery(&pool, run.id, "transport_disconnect")
                .await
                .expect("prepare recovered start")
        );
        assert!(
            claim_harness_start(&pool, run.id, "supervisor:second", stale_token, 120,)
                .await
                .expect("claim second start")
        );
        let mut second_tx = pool.begin().await.expect("begin second start");
        assert!(update_run_started(
            &mut second_tx,
            run.id,
            Some("thread-second"),
            Some("turn-second"),
            None,
            stale_token,
        )
        .await
        .expect("persist second start"));
        second_tx.commit().await.expect("commit second start");
        let stored = sqlx::query_as::<_, (String, String)>(
            "SELECT thread_id,turn_id FROM devrail_runs WHERE id=$1",
        )
        .bind(run.id)
        .fetch_one(&pool)
        .await
        .expect("read recovered run");
        assert_eq!(
            stored,
            ("thread-second".to_string(), "turn-second".to_string())
        );
        drop(pool);
        fixture.cleanup().await.expect("cleanup harness run schema");
    }

    #[tokio::test]
    async fn quality_gate_execution_claim_has_one_owner_until_release() {
        let Ok(fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = fixture.pool().clone();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let (owner_user_id, organization_id, department_id, task_id) =
            create_harness_test_task(&pool, &suffix)
                .await
                .expect("create task");
        let actor = ActorContext {
            actor_type: crate::access::ActorType::System,
            user_id: owner_user_id,
            session_id: 0,
            organization_id,
            department_id,
            data_scope: crate::access::DataScope::Organization,
            permission_codes: std::collections::BTreeSet::new(),
        };
        let dispatch_snapshot = sqlx::query_scalar::<_, Value>(
            "SELECT dispatch_snapshot FROM devrail_tasks WHERE id=$1",
        )
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("read task snapshot");
        let workflow_snapshot = dispatch_snapshot
            .get("workflow")
            .cloned()
            .expect("workflow snapshot");
        let mut tx = pool.begin().await.expect("begin gate claim fixture");
        let snapshot_id =
            create_snapshot(&mut tx, &actor, task_id, &dispatch_snapshot, department_id)
                .await
                .expect("create snapshot");
        let run = create_run(
            &mut tx,
            &NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: "quality-gate-claim",
                attempt: 1,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: &"0".repeat(64),
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: None,
                parent_turn_id: None,
                branch_name: None,
                branch_expires_at: None,
                cwd: "/tmp/devrail-quality-gate-claim",
                policy: &serde_json::json!({}),
                startup_args: &serde_json::json!(["app-server"]),
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("create run")
        .expect("run inserted");
        sqlx::query("UPDATE devrail_runs SET status='completed' WHERE id=$1")
            .bind(run.id)
            .execute(&mut *tx)
            .await
            .expect("complete run");
        tx.commit().await.expect("commit gate claim fixture");

        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        assert!(
            claim_quality_gate_execution(&pool, run.id, "gate:first", first, 300)
                .await
                .expect("claim first gate")
        );
        assert!(
            !claim_quality_gate_execution(&pool, run.id, "gate:second", second, 300)
                .await
                .expect("reject second gate")
        );
        assert!(release_quality_gate_execution(&pool, run.id, first)
            .await
            .expect("release first gate"));
        assert!(
            claim_quality_gate_execution(&pool, run.id, "gate:second", second, 300)
                .await
                .expect("claim after release")
        );
        assert!(release_quality_gate_execution(&pool, run.id, second)
            .await
            .expect("release second gate"));
        let mut terminal_tx = pool.begin().await.expect("begin terminal guard");
        assert!(mark_quality_gate_failed(&mut terminal_tx, run.id, task_id)
            .await
            .expect("mark first terminal failure"));
        assert!(!mark_quality_gate_failed(&mut terminal_tx, run.id, task_id)
            .await
            .expect("reject repeated terminal failure"));
        terminal_tx.commit().await.expect("commit terminal guard");
        drop(pool);
        fixture.cleanup().await.expect("cleanup gate claim schema");
    }
}

#[cfg(test)]
mod hook_failure_tests {
    use super::*;
    #[tokio::test]
    async fn hook_failure_count_tracks_same_fingerprint_and_resets() {
        let Ok(fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = fixture.pool().clone();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let (_, _, _, task_id) = create_harness_test_task(&pool, &suffix)
            .await
            .expect("create task");

        for expected in 1..=5 {
            let mut tx = pool.begin().await.expect("begin hook failure transaction");
            let count = record_hook_failure(&mut tx, task_id, "before-run-hook-error")
                .await
                .expect("record hook failure");
            tx.commit().await.expect("commit hook failure transaction");
            assert_eq!(count, expected);
            assert_eq!(
                hook_failure_breaker_open(count),
                expected >= MAX_HOOK_FAILURES
            );
        }

        let mut tx = pool
            .begin()
            .await
            .expect("begin fingerprint reset transaction");
        assert_eq!(
            record_hook_failure(&mut tx, task_id, "different-hook-error")
                .await
                .expect("record changed hook failure"),
            1
        );
        clear_hook_failure(&mut tx, task_id)
            .await
            .expect("clear hook failure");
        tx.commit()
            .await
            .expect("commit fingerprint reset transaction");

        let state = sqlx::query_as::<_, (Option<String>, i32)>(
            "SELECT hook_failure_fingerprint, hook_failure_count
             FROM devrail_tasks WHERE id=$1",
        )
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("read hook failure state");
        assert_eq!(state, (None, 0));
        drop(pool);
        fixture
            .cleanup()
            .await
            .expect("cleanup hook failure schema");
    }
}
