//! Scoped persistence for Harness Supervisor runs and their append-only events.

use crate::access::ActorContext;
use crate::models::{DevRailRunEventRow, DevRailRunRow};
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const RUN_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, attempt, task_revision, workflow_source, workflow_version, workflow_digest, workflow_snapshot, actor_type, last_heartbeat_at, last_event_at, retry_reason, parent_run_id, parent_turn_id, cleanup_status, branch_name, branch_expires_at, status, thread_id, turn_id, harness_version, model_id, cwd, policy, startup_args_summary, exit_reason, exit_code, stderr_summary, trace_id, recovery_suggestion, recovery_attempts, started_at, completed_at, created_at, updated_at";
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
        "INSERT INTO devrail_runs (organization_id, department_id, owner_user_id, task_id, snapshot_id, idempotency_key, attempt, task_revision, workflow_source, workflow_version, workflow_digest, workflow_snapshot, actor_type, parent_run_id, parent_turn_id, branch_name, branch_expires_at, status, cwd, policy, startup_args_summary, model_id)
         SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,'starting',$18,$19,$20,$21
         FROM devrail_tasks t
         WHERE t.id=$4 AND t.organization_id=$1 AND t.revision=$8
           AND t.workflow_source=$9 AND t.workflow_version=$10 AND t.workflow_digest=$11
           AND t.dispatch_snapshot->'workflow'=$12::jsonb
         ON CONFLICT DO NOTHING RETURNING {RUN_COLUMNS}"
    );
    sqlx::query_as::<_, DevRailRunRow>(AssertSqlSafe(sql))
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
        .fetch_optional(c)
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
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devrail_runs SET status='active', thread_id=COALESCE($2,thread_id), turn_id=COALESCE($3,turn_id), harness_version=COALESCE($4,harness_version), started_at=COALESCE(started_at,now()), last_heartbeat_at=now(), updated_at=now() WHERE id=$1 AND status IN ('created','starting','active')")
        .bind(run_id).bind(thread_id).bind(turn_id).bind(harness_version).execute(c).await.map(|_| ())
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

pub(crate) async fn mark_unrecoverable_runs(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let rows = sqlx::query_as::<_, (i64,)>("UPDATE devrail_runs SET status='failed', exit_reason='supervisor_restart', retry_reason='服务重启后缺少可恢复 thread', recovery_suggestion='服务重启后运行无法自动恢复；请使用相同快照重试', completed_at=COALESCE(completed_at,now()), cleanup_status='completed', updated_at=now() WHERE status IN ('starting','active') AND thread_id IS NULL RETURNING id")
        .fetch_all(&mut *tx)
        .await?;
    if !rows.is_empty() {
        let run_ids = rows.iter().map(|row| row.0).collect::<Vec<_>>();
        sqlx::query("UPDATE devrail_tasks t SET status='failed', scheduler_claim_token=NULL, scheduler_claimed_at=NULL, updated_at=now() WHERE t.status='running' AND EXISTS (SELECT 1 FROM devrail_runs r WHERE r.task_id=t.id AND r.status='failed' AND r.exit_reason='supervisor_restart')")
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
    use crate::db::DATABASE_TEST_LOCK;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn run_insert_requires_and_copies_exact_task_workflow_identity() {
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let pool = crate::db::init_pool(&database_url)
            .await
            .expect("connect test database");
        crate::db::run_migrations(&pool)
            .await
            .expect("run migrations");
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
    }
}
