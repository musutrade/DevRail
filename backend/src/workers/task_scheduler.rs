//! Symphony-style scheduler for queued DevRail tasks.
//!
//! The scheduler is deliberately small: PostgreSQL owns queue ordering and
//! leases, while the Harness Supervisor remains the only component that can
//! start a Codex process.

use crate::access::{ActorContext, ActorType, DataScope};
use crate::error::ApiError;
use crate::models::{CreateDevRailRunRequest, DevRailTaskRow};
use crate::repositories::devrail;
use crate::services::devrail_runs;
use crate::workers::harness_supervisor::HarnessSupervisor;
use chrono::{Duration as ChronoDuration, Utc};
use sqlx::PgPool;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const CLAIM_BATCH_SIZE: i64 = 16;

#[derive(Debug, Clone, Copy)]
pub struct SchedulerPolicy {
    pub poll_interval: Duration,
    pub claim_lease_seconds: i64,
    pub retry_base_seconds: i64,
    pub retry_max_seconds: i64,
    pub retry_jitter_percent: i64,
    pub stall_timeout: Duration,
    pub priority_aging_seconds: i64,
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(10),
            claim_lease_seconds: 60,
            retry_base_seconds: 1,
            retry_max_seconds: 300,
            retry_jitter_percent: 20,
            stall_timeout: Duration::from_secs(120),
            priority_aging_seconds: 3_600,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickPhase {
    Reconcile,
    Dispatch,
    ReapMetrics,
}

const TICK_PHASES: [TickPhase; 3] = [
    TickPhase::Reconcile,
    TickPhase::Dispatch,
    TickPhase::ReapMetrics,
];

pub fn spawn(
    pool: PgPool,
    supervisor: Arc<HarnessSupervisor>,
    policy: SchedulerPolicy,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(policy.poll_interval);
        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => {
                    tracing::info!("DevRail task scheduler stopped gracefully");
                    break;
                }
                _ = interval.tick() => {
                    if let Err(error) = run_tick(&pool, &supervisor, policy).await {
                        tracing::error!(error = %error, "DevRail task scheduler tick failed");
                    }
                }
            }
        }
    })
}

async fn run_tick(
    pool: &PgPool,
    supervisor: &HarnessSupervisor,
    policy: SchedulerPolicy,
) -> Result<(), sqlx::Error> {
    debug_assert_eq!(TICK_PHASES[0], TickPhase::Reconcile);
    let running_run_ids = supervisor.running_run_ids().await;
    let reconciliation = devrail::reconcile_scheduler_state(
        pool,
        &running_run_ids,
        policy.stall_timeout.as_secs() as i64,
    )
    .await?;
    for pending in &reconciliation.pending_interruptions {
        match supervisor
            .interrupt_for_reconciliation(pending.run_id, &pending.reason)
            .await
        {
            Ok(()) => crate::app_metrics::record_reconciliation(&pending.reason),
            Err(crate::workers::harness_supervisor::SupervisorError::ControlUnavailable) => {
                tracing::debug!(
                    run_id = pending.run_id,
                    reason = pending.reason,
                    "pending interruption is owned by another Supervisor instance"
                );
            }
            Err(error) => {
                tracing::warn!(
                    run_id = pending.run_id,
                    reason = pending.reason,
                    error = %error,
                    "failed to propagate scheduler interruption"
                );
            }
        }
    }
    crate::app_metrics::record_scheduler_queue_depth(reconciliation.queue_depth);
    crate::app_metrics::record_active_runs(reconciliation.active_runs);
    if reconciliation.released_claims > 0 {
        crate::app_metrics::record_reconciliation("released_claim");
    }
    if reconciliation.stale_runs > 0 {
        crate::app_metrics::record_scheduler_stall();
        crate::app_metrics::record_reconciliation("stale_run");
    }
    if reconciliation.exhausted_tasks > 0 {
        crate::app_metrics::record_reconciliation("retry_exhausted");
    }
    crate::app_metrics::record_reconciliation("ok");
    let claim_token = Uuid::new_v4();
    let tasks = devrail::claim_scheduler_tasks(
        pool,
        claim_token,
        CLAIM_BATCH_SIZE,
        policy.claim_lease_seconds,
        policy.priority_aging_seconds,
    )
    .await?;
    if tasks.is_empty() {
        crate::app_metrics::record_scheduler_dispatch("empty");
        return Ok(());
    }
    tracing::debug!(
        claimed_tasks = tasks.len(),
        "DevRail scheduler claimed queued tasks"
    );
    for task in tasks {
        if !devrail::renew_scheduler_claim(pool, task.id, claim_token, policy.claim_lease_seconds)
            .await?
        {
            crate::app_metrics::record_scheduler_claim_conflict();
            crate::app_metrics::record_scheduler_dispatch("stale_claim");
            continue;
        }
        if let Err(error) = dispatch_task(pool, supervisor, &task, claim_token).await {
            let reason = dispatch_failure_reason(&error);
            tracing::warn!(
                task_id = task.id,
                reason,
                "DevRail scheduler could not dispatch task"
            );
            if matches!(error, ApiError::Conflict(ref message) if message == "Harness 并发额度已用尽")
            {
                crate::app_metrics::record_scheduler_dispatch("capacity");
                let _ = devrail::release_scheduler_claim(pool, task.id, claim_token).await;
            } else if is_retryable(&error) && task.scheduler_attempt < task.scheduler_max_attempts {
                crate::app_metrics::record_scheduler_dispatch("failed");
                let retry_at = Utc::now() + retry_delay(task.scheduler_attempt, policy);
                if devrail::schedule_retry(pool, task.id, claim_token, retry_at, reason)
                    .await
                    .unwrap_or(false)
                {
                    crate::app_metrics::record_scheduler_retry();
                } else {
                    let _ = devrail::release_scheduler_claim(pool, task.id, claim_token).await;
                }
            } else if devrail::fail_scheduler_task(pool, task.id, claim_token, reason)
                .await
                .unwrap_or(false)
            {
                crate::app_metrics::record_scheduler_dispatch("permanent_failure");
            } else {
                let _ = devrail::release_scheduler_claim(pool, task.id, claim_token).await;
            }
        } else {
            crate::app_metrics::record_scheduler_dispatch("started");
            let queue_seconds = Utc::now()
                .signed_duration_since(task.created_at)
                .num_milliseconds()
                .max(0) as f64
                / 1000.0;
            crate::app_metrics::record_scheduler_dispatch_latency(queue_seconds);
        }
    }
    debug_assert_eq!(TICK_PHASES[2], TickPhase::ReapMetrics);
    Ok(())
}

pub(crate) fn retry_delay(attempt: i32, policy: SchedulerPolicy) -> ChronoDuration {
    let mut random = [0_u8; 8];
    let seed = if getrandom::fill(&mut random).is_ok() {
        u64::from_le_bytes(random)
    } else {
        u64::from(attempt.unsigned_abs())
    };
    retry_delay_with_seed(attempt, policy, seed)
}

fn retry_delay_with_seed(
    attempt: i32,
    policy: SchedulerPolicy,
    jitter_seed: u64,
) -> ChronoDuration {
    let bounded_attempt = i64::from(attempt.clamp(1, 8));
    let base_seconds = policy
        .retry_base_seconds
        .saturating_mul(2_i64.saturating_pow((bounded_attempt - 1) as u32))
        .min(policy.retry_max_seconds);
    let jitter_bound = base_seconds
        .saturating_mul(policy.retry_jitter_percent)
        .checked_div(100)
        .unwrap_or_default();
    let jitter_range = u64::try_from(jitter_bound.saturating_add(1)).unwrap_or(1);
    let jitter_seconds = i64::try_from(jitter_seed % jitter_range).unwrap_or_default();
    ChronoDuration::seconds(
        base_seconds
            .saturating_add(jitter_seconds)
            .min(policy.retry_max_seconds),
    )
}

fn is_retryable(error: &ApiError) -> bool {
    matches!(error, ApiError::Internal(_) | ApiError::Conflict(_))
}

fn dispatch_failure_reason(error: &ApiError) -> &'static str {
    match error {
        ApiError::Unauthorized | ApiError::Forbidden(_) => "调度权限或安全策略不允许执行",
        ApiError::NotFound(_) => "调度依赖资源不存在",
        ApiError::Validation(_) => "任务配置校验失败",
        ApiError::Conflict(_) => "Harness 或任务状态暂时冲突",
        ApiError::Internal(_) => "Harness 或数据库暂时不可用",
        ApiError::CsrfInvalid | ApiError::RateLimited { .. } => "调度请求被安全策略拒绝",
    }
}

async fn dispatch_task(
    pool: &PgPool,
    supervisor: &HarnessSupervisor,
    task: &DevRailTaskRow,
    claim_token: Uuid,
) -> Result<(), ApiError> {
    let environment_id = task
        .environment_id
        .ok_or_else(|| ApiError::validation("排队任务缺少运行环境"))?;
    let actor = scheduler_actor(task);
    let request = CreateDevRailRunRequest {
        environment_id,
        idempotency_key: scheduler_idempotency_key(task.id, task.scheduler_attempt),
        model_id: None,
        input: None,
        branch_name: None,
    };
    match devrail_runs::create_scheduled_run(
        pool,
        &actor,
        supervisor,
        task.id,
        &request,
        claim_token,
    )
    .await
    {
        Ok(run) => {
            tracing::info!(
                task_id = task.id,
                run_id = run.id,
                "DevRail scheduler started task"
            );
            Ok(())
        }
        Err(ApiError::Conflict(message)) if message == "Harness 并发额度已用尽" => {
            // Capacity is transient. The claim is released and the task stays
            // queued for the next tick rather than being reported as failed.
            Err(ApiError::conflict(message))
        }
        Err(error) => Err(error),
    }
}

fn scheduler_actor(task: &DevRailTaskRow) -> ActorContext {
    ActorContext {
        actor_type: ActorType::System,
        user_id: task.owner_user_id,
        session_id: 0,
        organization_id: task.organization_id,
        department_id: task.department_id,
        data_scope: DataScope::Organization,
        permission_codes: BTreeSet::from([
            "devrail:task:read".to_string(),
            "devrail:run:execute".to_string(),
        ]),
    }
}

fn scheduler_idempotency_key(task_id: i64, attempt: i32) -> String {
    format!("scheduler:{task_id}:{attempt}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;

    fn task() -> DevRailTaskRow {
        DevRailTaskRow {
            id: 42,
            organization_id: 7,
            department_id: Some(3),
            owner_user_id: 9,
            project_id: 11,
            repository_id: None,
            environment_id: Some(13),
            assignee_user_id: None,
            title: "任务".to_string(),
            goal: "目标".to_string(),
            background: None,
            acceptance_criteria: None,
            constraints: None,
            priority: "normal".to_string(),
            status: "queued".to_string(),
            scheduler_attempt: 0,
            scheduler_retry_count: 0,
            scheduler_max_attempts: 3,
            scheduler_retry_at: None,
            scheduler_last_error: None,
            labels: json!([]),
            due_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
        }
    }

    #[test]
    fn scheduler_key_is_bounded_and_reproducible() {
        let first = scheduler_idempotency_key(42, 1);
        assert_eq!(first, scheduler_idempotency_key(42, 1));
        assert!(first.len() <= 128);
        assert_eq!(first, "scheduler:42:1");
    }

    #[test]
    fn retry_delay_is_bounded_and_increases_with_attempt() {
        let policy = SchedulerPolicy::default();
        assert_eq!(
            retry_delay_with_seed(1, policy, 0),
            ChronoDuration::seconds(1)
        );
        assert_eq!(
            retry_delay_with_seed(2, policy, u64::MAX),
            ChronoDuration::seconds(2)
        );
        let bounded_policy = SchedulerPolicy {
            retry_base_seconds: 10,
            retry_max_seconds: 100,
            retry_jitter_percent: 20,
            ..policy
        };
        let jittered = retry_delay_with_seed(1, bounded_policy, u64::MAX);
        assert!(jittered >= ChronoDuration::seconds(10));
        assert!(jittered <= ChronoDuration::seconds(12));
        assert_eq!(
            retry_delay_with_seed(99, bounded_policy, u64::MAX),
            ChronoDuration::seconds(100)
        );
        assert!(retry_delay_with_seed(-1, policy, 0) > ChronoDuration::zero());
    }

    #[test]
    fn dispatch_failures_distinguish_transient_and_permanent_errors() {
        assert!(is_retryable(&ApiError::internal(
            "temporary database outage"
        )));
        assert!(is_retryable(&ApiError::conflict("capacity")));
        assert!(!is_retryable(&ApiError::validation("missing environment")));
        assert!(!is_retryable(&ApiError::forbidden("policy denied")));
        assert_eq!(
            dispatch_failure_reason(&ApiError::validation("secret")),
            "任务配置校验失败"
        );
    }

    #[test]
    fn scheduler_actor_uses_task_scope_without_a_user_session() {
        let actor = scheduler_actor(&task());
        assert_eq!(actor.actor_type, ActorType::System);
        assert_eq!(actor.organization_id, 7);
        assert_eq!(actor.data_scope, DataScope::Organization);
        assert!(actor.has_permission("devrail:run:execute"));
    }

    #[test]
    fn tick_phase_order_is_reconcile_dispatch_reap() {
        assert_eq!(
            TICK_PHASES,
            [
                TickPhase::Reconcile,
                TickPhase::Dispatch,
                TickPhase::ReapMetrics
            ]
        );
    }

    #[tokio::test]
    async fn cancelled_scheduler_stops_without_polling_the_database() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://devrail:unused@127.0.0.1:1/devrail")
            .expect("lazy test pool");
        let supervisor = Arc::new(HarnessSupervisor::new(
            pool.clone(),
            "codex".to_string(),
            1,
            60,
            "/tmp/devrail-workspaces".to_string(),
            1,
            SchedulerPolicy::default(),
        ));
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let worker = spawn(pool, supervisor, SchedulerPolicy::default(), shutdown);
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .expect("scheduler shutdown must not block")
            .expect("scheduler worker must join cleanly");
    }
}
