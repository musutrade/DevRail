//! Symphony-style scheduler for queued DevRail tasks.
//!
//! The scheduler is deliberately small: PostgreSQL owns queue ordering and
//! leases, while the Harness Supervisor remains the only component that can
//! start a Codex process.

use crate::access::{ActorContext, ActorType, DataScope};
use crate::error::ApiError;
use crate::models::{
    ContinuationPolicy, CreateDevRailRunRequest, DevRailContinuationTrigger,
    DevRailRepairRequestRow, DevRailTaskRow, RepairPolicy,
};
use crate::orchestration::task_tracker::{PostgresTaskTracker, TaskTracker, TrackerError};
use crate::repositories;
use crate::services::devrail_workspaces;
use crate::services::{devrail_repairs, devrail_runs};
use crate::workers::harness_supervisor::{HarnessSupervisor, RunLaunch};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchPhase {
    Continuations,
    DispatchedContinuationRecovery,
    Repairs,
    DispatchedRepairRecovery,
    RepairGateReruns,
    QueuedTasks,
}

const TICK_PHASES: [TickPhase; 3] = [
    TickPhase::Reconcile,
    TickPhase::Dispatch,
    TickPhase::ReapMetrics,
];

const DISPATCH_PHASES: [DispatchPhase; 6] = [
    DispatchPhase::Continuations,
    DispatchPhase::DispatchedContinuationRecovery,
    DispatchPhase::Repairs,
    DispatchPhase::DispatchedRepairRecovery,
    DispatchPhase::RepairGateReruns,
    DispatchPhase::QueuedTasks,
];

pub fn spawn(
    pool: PgPool,
    supervisor: Arc<HarnessSupervisor>,
    policy: SchedulerPolicy,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let tracker = Arc::new(PostgresTaskTracker::new(pool.clone()));
    spawn_with_tracker(pool, tracker, supervisor, policy, shutdown)
}

fn spawn_with_tracker(
    pool: PgPool,
    tracker: Arc<dyn TaskTracker>,
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
                    if let Err(error) = run_tick(&pool, tracker.as_ref(), &supervisor, policy, true).await {
                        tracing::error!(error_kind = ?error.kind(), error = %error, "DevRail task scheduler tick failed");
                    }
                }
            }
        }
    })
}

async fn run_tick(
    pool: &PgPool,
    tracker: &dyn TaskTracker,
    supervisor: &HarnessSupervisor,
    policy: SchedulerPolicy,
    database_side_channels: bool,
) -> Result<(), TrackerError> {
    debug_assert_eq!(TICK_PHASES[0], TickPhase::Reconcile);
    let dependency_propagations = tracker.reconcile_dependencies().await?;
    if dependency_propagations > 0 {
        crate::app_metrics::record_dependency_propagation("applied", dependency_propagations);
    }
    let running_run_ids = supervisor.running_run_ids().await;
    let reconciliation = tracker
        .reconcile(&running_run_ids, policy.stall_timeout.as_secs() as i64)
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
    if database_side_channels {
        match repositories::devrail_continuations::release_expired_claims(pool, 500).await {
            Ok(released) if released > 0 => {
                crate::app_metrics::record_reconciliation("continuation_claim_released");
                crate::app_metrics::record_continuation_event("recovered", "pending", "other");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error = %error, "continuation claim expiry reconciliation failed");
                crate::app_metrics::record_reconciliation("continuation_claim_release_failed");
            }
        }
        match repositories::devrail_repairs::release_expired_claims(pool, 500).await {
            Ok(released) if released > 0 => {
                crate::app_metrics::record_reconciliation("repair_claim_released");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error = %error, "repair claim expiry reconciliation failed");
                crate::app_metrics::record_reconciliation("repair_claim_release_failed");
            }
        }
        if let Ok(depth) = repositories::devrail_continuations::pending_depth(pool).await {
            crate::app_metrics::record_continuation_pending(depth);
        }
    }
    crate::app_metrics::record_reconciliation("ok");
    if database_side_channels {
        if let Err(error) =
            devrail_workspaces::reconcile_cleanup(pool, &supervisor.workspace_root()).await
        {
            tracing::warn!(error = %error, "workspace cleanup reconciliation failed");
            crate::app_metrics::record_reconciliation("workspace_cleanup_failed");
        }
    }
    debug_assert_eq!(DISPATCH_PHASES[0], DispatchPhase::Continuations);
    if database_side_channels {
        dispatch_continuations(pool, supervisor).await;
        debug_assert_eq!(
            DISPATCH_PHASES[1],
            DispatchPhase::DispatchedContinuationRecovery
        );
        reconcile_dispatched_continuations(pool, supervisor).await;
        debug_assert_eq!(DISPATCH_PHASES[2], DispatchPhase::Repairs);
        dispatch_repairs(pool, supervisor).await;
        debug_assert_eq!(DISPATCH_PHASES[3], DispatchPhase::DispatchedRepairRecovery);
        reconcile_dispatched_repairs(pool, supervisor).await;
        debug_assert_eq!(DISPATCH_PHASES[4], DispatchPhase::RepairGateReruns);
        dispatch_repair_gate_reruns(pool, supervisor).await;
    }
    debug_assert_eq!(DISPATCH_PHASES[5], DispatchPhase::QueuedTasks);
    let claim_token = Uuid::new_v4();
    let tasks = tracker
        .claim_dispatch_candidates(
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
    for claimed_task in tasks {
        let actor = scheduler_actor(&claimed_task);
        let Some(task) = tracker.find_task(&actor, claimed_task.id).await? else {
            crate::app_metrics::record_scheduler_dispatch("stale_claim");
            let _ = tracker.release_claim(claimed_task.id, claim_token).await;
            continue;
        };
        if !tracker
            .renew_claim(task.id, claim_token, policy.claim_lease_seconds)
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
                let _ = tracker.release_claim(task.id, claim_token).await;
            } else if is_retryable(&error) && task.scheduler_attempt < task.scheduler_max_attempts {
                crate::app_metrics::record_scheduler_dispatch("failed");
                let retry_at = Utc::now() + retry_delay(task.scheduler_attempt, policy);
                if tracker
                    .schedule_retry(task.id, claim_token, retry_at, reason)
                    .await
                    .unwrap_or(false)
                {
                    crate::app_metrics::record_scheduler_retry();
                } else {
                    let _ = tracker.release_claim(task.id, claim_token).await;
                }
            } else if tracker
                .fail_task(task.id, claim_token, reason)
                .await
                .unwrap_or(false)
            {
                crate::app_metrics::record_scheduler_dispatch("permanent_failure");
            } else {
                let _ = tracker.release_claim(task.id, claim_token).await;
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

async fn dispatch_repair_gate_reruns(pool: &PgPool, supervisor: &HarnessSupervisor) {
    match repositories::devrail_repairs::release_expired_gate_rerun_claims(pool, 500).await {
        Ok(released) if released > 0 => {
            crate::app_metrics::record_reconciliation("repair_gate_claim_released");
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(error = %error, "repair gate claim expiry reconciliation failed");
            crate::app_metrics::record_reconciliation("repair_gate_claim_release_failed");
        }
    }
    let worker_id = format!("repair-gate-scheduler:{}", Uuid::new_v4().simple());
    let claim_token = Uuid::new_v4();
    let reruns = match repositories::devrail_repairs::claim_gate_reruns(
        pool,
        &worker_id,
        claim_token,
        CLAIM_BATCH_SIZE,
        supervisor.scheduler_policy().claim_lease_seconds,
    )
    .await
    {
        Ok(reruns) => reruns,
        Err(error) => {
            tracing::warn!(error = %error, "repair gate claim failed");
            crate::app_metrics::record_reconciliation("repair_gate_claim_failed");
            return;
        }
    };
    for rerun in reruns {
        let rerun_id = rerun.id;
        match devrail_repairs::execute_gate_rerun(pool, &worker_id, claim_token, rerun).await {
            Ok(()) => crate::app_metrics::record_reconciliation("repair_gate_completed"),
            Err(error) => {
                tracing::warn!(error = %error, "repair gate rerun failed");
                let _ = repositories::devrail_repairs::release_gate_rerun_claim(
                    pool,
                    rerun_id,
                    &worker_id,
                    claim_token,
                )
                .await;
                crate::app_metrics::record_reconciliation("repair_gate_failed");
            }
        }
    }
}

async fn reconcile_dispatched_repairs(pool: &PgPool, supervisor: &HarnessSupervisor) {
    let requests = match repositories::devrail_repairs::list_dispatched_unstarted(pool, 100).await {
        Ok(requests) => requests,
        Err(error) => {
            tracing::warn!(error = %error, "repair dispatch reconciliation failed");
            crate::app_metrics::record_reconciliation("repair_dispatch_reconcile_failed");
            return;
        }
    };
    for request in requests {
        let Some(child_run_id) = request.child_run_id else {
            continue;
        };
        if supervisor.running_run_ids().await.contains(&child_run_id) {
            continue;
        }
        let Some(child) = repositories::devrail_runs::find_for_recovery(pool, child_run_id)
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        let launch = RunLaunch {
            run_id: child.id,
            task_id: child.task_id,
            organization_id: child.organization_id,
            department_id: child.department_id,
            owner_user_id: child.owner_user_id,
            cwd: std::path::PathBuf::from(&child.cwd),
            input: format!(
                "请根据失败诊断执行第 {} 次受控修复",
                request.repair_sequence
            ),
            resume_thread_id: None,
            resume_turn_id: None,
            attempt: child.attempt,
            max_attempts: child.attempt.saturating_add(1),
            automatic: true,
            scheduler_policy: supervisor.scheduler_policy(),
        };
        if let Err(error) = supervisor.launch(launch).await {
            tracing::warn!(child_run_id, error = %error, "repair child launch reconciliation failed");
            crate::app_metrics::record_reconciliation("repair_launch_failed");
        } else {
            crate::app_metrics::record_reconciliation("repair_launch_recovered");
        }
    }
}

async fn dispatch_repairs(pool: &PgPool, supervisor: &HarnessSupervisor) {
    let worker_id = format!("repair-scheduler:{}", Uuid::new_v4().simple());
    let claim_token = Uuid::new_v4();
    let requests = match repositories::devrail_repairs::claim_pending(
        pool,
        &worker_id,
        claim_token,
        CLAIM_BATCH_SIZE,
        supervisor.scheduler_policy().claim_lease_seconds,
    )
    .await
    {
        Ok(requests) => requests,
        Err(error) => {
            tracing::warn!(error = %error, "repair claim failed");
            crate::app_metrics::record_reconciliation("repair_claim_failed");
            return;
        }
    };
    for request in requests {
        let request_id = request.id;
        let attempts = request.dispatch_attempts;
        let risk_category = request.risk_category.clone();
        let actor = scheduler_actor_for_repair(&request);
        let dispatch_started = std::time::Instant::now();
        crate::app_metrics::record_repair_request("claimed", "claimed", &risk_category);
        if let Err(error) =
            dispatch_repair(pool, supervisor, &worker_id, claim_token, request).await
        {
            tracing::warn!(request_id, error = %error, "repair dispatch failed");
            if matches!(&error, ApiError::Conflict(message) if message.contains("claim") || message.contains("其他 worker"))
            {
                crate::app_metrics::record_repair_claim_conflict();
            }
            if repair_dispatch_is_deterministic(&error) {
                let reason = repair_dispatch_reason(&error);
                if reason == "budget_exceeded" {
                    crate::app_metrics::record_repair_budget_rejected();
                }
                if reason == "hook_failure_circuit_open" {
                    crate::app_metrics::record_repair_hook_circuit();
                }
                if let Ok(mut tx) = pool.begin().await {
                    if repositories::devrail_repairs::handoff(
                        &mut tx,
                        &actor,
                        request_id,
                        &repositories::devrail_repairs::NewRepairHandoff {
                            reason_code: reason,
                            recommendation:
                                "修复无法自动派发，请由授权人员检查诊断、审批和运行环境。",
                        },
                    )
                    .await
                    .is_ok()
                    {
                        let _ = tx.commit().await;
                        crate::app_metrics::record_repair_handoff(reason);
                        crate::app_metrics::record_repair_request(
                            "handed_off",
                            "handed_off",
                            &risk_category,
                        );
                    }
                }
            } else {
                let backoff = retry_delay(attempts, supervisor.scheduler_policy()).num_seconds();
                let _ = repositories::devrail_repairs::release_claim(
                    pool,
                    request_id,
                    &worker_id,
                    claim_token,
                    backoff,
                )
                .await;
            }
        } else {
            crate::app_metrics::record_reconciliation("repair_dispatched");
            crate::app_metrics::record_repair_request("dispatched", "dispatched", &risk_category);
            crate::app_metrics::record_repair_dispatch_latency(
                dispatch_started.elapsed().as_secs_f64(),
            );
        }
    }
}

fn repair_dispatch_is_deterministic(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::NotFound(_) | ApiError::Validation(_) | ApiError::Forbidden(_)
    ) || matches!(error, ApiError::Conflict(message) if message.contains("不存在") || message.contains("不匹配") || message.contains("不允许") || message.contains("超过") || message.contains("已禁用") || message.contains("已过期") || message.contains("审批") || message.contains("达到") || message.contains("Hook") || message.contains("熔断"))
}

fn repair_dispatch_reason(error: &ApiError) -> &'static str {
    match error {
        ApiError::Unavailable => "dispatch_unavailable",
        ApiError::NotFound(_) => "source_missing",
        ApiError::Validation(_) => "validation_rejected",
        ApiError::Forbidden(_) => "policy_rejected",
        ApiError::Conflict(message) if message.contains("审批") => "approval_required",
        ApiError::Conflict(message) if message.contains("过期") => "evidence_expired",
        ApiError::Conflict(message) if message.contains("Hook") => "hook_failure_circuit_open",
        ApiError::Conflict(message) if message.contains("成本") => "budget_exceeded",
        ApiError::Conflict(_) => "dispatch_prerequisite_invalid",
        _ => "dispatch_prerequisite_invalid",
    }
}

fn scheduler_actor_for_repair(request: &DevRailRepairRequestRow) -> ActorContext {
    ActorContext {
        actor_type: ActorType::System,
        user_id: request.owner_user_id,
        session_id: 0,
        organization_id: request.organization_id,
        department_id: request.department_id,
        data_scope: DataScope::All,
        permission_codes: BTreeSet::new(),
    }
}

async fn dispatch_repair(
    pool: &PgPool,
    supervisor: &HarnessSupervisor,
    worker_id: &str,
    claim_token: Uuid,
    request: DevRailRepairRequestRow,
) -> Result<(), ApiError> {
    let actor = scheduler_actor_for_repair(&request);
    let policy: RepairPolicy = serde_json::from_value(request.policy_snapshot.clone())
        .map_err(|_| ApiError::conflict("repair 固化策略不可用"))?;
    if !policy.enabled {
        return Err(ApiError::conflict("repair 固化策略已禁用"));
    }
    if request.dispatch_attempts > policy.max_dispatch_attempts {
        return Err(ApiError::conflict("repair 派发次数超过固化策略"));
    }
    let task = repositories::devrail::find_task_by_id(pool, &actor, request.task_id)
        .await
        .map_err(crate::error::db_error)?
        .ok_or_else(|| ApiError::not_found("repair 任务不存在"))?;
    let current_repair_request_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT current_repair_request_id FROM devrail_tasks WHERE id=$1 AND organization_id=$2",
    )
    .bind(task.id)
    .bind(task.organization_id)
    .fetch_optional(pool)
    .await
    .map_err(crate::error::db_error)?
    .flatten();
    if task.status != "repair_pending" || current_repair_request_id != Some(request.id) {
        return Err(ApiError::conflict("repair 任务状态或范围不匹配"));
    }
    if task.hook_failure_count >= repositories::devrail_runs::MAX_HOOK_FAILURES {
        return Err(ApiError::conflict("Hook 失败熔断已打开"));
    }
    let source = repositories::devrail_runs::find_for_recovery(pool, request.source_run_id)
        .await
        .map_err(crate::error::db_error)?
        .ok_or_else(|| ApiError::not_found("repair 来源运行不存在"))?;
    if source.status != "failed" || source.task_id != request.task_id {
        return Err(ApiError::conflict("repair 来源运行状态或谱系不匹配"));
    }
    if !repositories::devrail_repairs::dispatch_evidence_is_current(pool, &actor, request.id)
        .await
        .map_err(crate::error::db_error)?
    {
        return Err(ApiError::conflict(
            "repair 诊断证据已过期或 changeset 不匹配",
        ));
    }
    if !repositories::devrail_repairs::approval_is_current(
        pool,
        &actor,
        request.id,
        &request.risk_category,
    )
    .await
    .map_err(crate::error::db_error)?
    {
        return Err(ApiError::conflict("repair 审批未满足、已撤回或已过期"));
    }
    let environment_id = task
        .environment_id
        .ok_or_else(|| ApiError::validation("repair 任务缺少运行环境"))?;
    let environment =
        repositories::devrail::find_environment(pool, &actor, task.project_id, environment_id)
            .await
            .map_err(crate::error::db_error)?
            .ok_or_else(|| ApiError::not_found("repair 运行环境不存在"))?;
    if !environment.enabled {
        return Err(ApiError::conflict("repair 运行环境已禁用"));
    }
    let reservation = supervisor
        .reserve()
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let relative = devrail_workspaces::repair_workspace_key(task.id, request.repair_sequence)?;
    let materialized = devrail_workspaces::materialize_repair_from_source(
        &supervisor.workspace_root(),
        std::path::Path::new(&environment.workspace_root),
        &relative,
        None,
    )
    .await?;
    let child_key = format!("repair:{}", request.id);
    let start_key = format!("repair:{}:start", request.id);
    let dispatch_result = async {
        let mut tx = pool.begin().await.map_err(crate::error::db_error)?;
        let child = repositories::devrail_runs::create_repair_run(
            &mut tx,
            &repositories::devrail_runs::NewRepairRun {
                actor: &actor,
                task_id: task.id,
                snapshot_id: source.snapshot_id,
                idempotency_key: &child_key,
                task_revision: task.revision,
                workflow_source: &source.workflow_source,
                workflow_version: &source.workflow_version,
                workflow_digest: &source.workflow_digest,
                workflow_snapshot: &source.workflow_snapshot,
                parent_run_id: request.source_run_id,
                parent_turn_id: source.turn_id.as_deref(),
                repair_request_id: request.id,
                repair_sequence: request.repair_sequence,
                harness_start_key: &start_key,
                cwd: materialized.path.to_string_lossy().as_ref(),
                policy: &request.policy_snapshot,
                startup_args: &serde_json::json!(["app-server"]),
                model_id: source.model_id.as_deref(),
                department_id: request.department_id,
            },
        )
        .await
        .map_err(crate::error::db_error)?
        .ok_or_else(|| ApiError::conflict("repair child run 已由其他 worker 创建"))?;
        let relative_digest = devrail_workspaces::path_digest(&relative);
        let workspace = repositories::devrail_workspaces::create(
            &mut tx,
            &repositories::devrail_workspaces::NewWorkspace {
                actor: &actor,
                task_id: task.id,
                run_id: Some(child.id),
                attempt: child.attempt,
                workspace_key: &relative,
                relative_path: &relative,
                path_digest: &relative_digest,
                repository_id: task.repository_id,
                environment_id: Some(environment.id),
                base_commit: materialized.base_commit.as_deref(),
                branch_name: None,
                workflow_version: Some(&source.workflow_version),
                workflow_digest: Some(&source.workflow_digest),
                environment_version: None,
                tool_versions: &serde_json::json!({}),
                snapshot_digest: Some(&request.failure_evidence_digest),
            },
        )
        .await
        .map_err(crate::error::db_error)?
        .ok_or_else(|| ApiError::conflict("repair workspace 已被其他请求占用"))?;
        repositories::devrail_workspaces::set_lifecycle(
            &mut tx,
            workspace.id,
            "running",
            "pending",
            Some("before_run"),
            None,
        )
        .await
        .map_err(crate::error::db_error)?;
        repositories::devrail_repairs::mark_dispatched(
            &mut tx,
            &actor,
            request.id,
            worker_id,
            claim_token,
            child.id,
        )
        .await
        .map_err(crate::error::db_error)?
        .ok_or_else(|| ApiError::conflict("repair claim 已失效"))?;
        tx.commit().await.map_err(crate::error::db_error)?;
        Ok::<_, ApiError>(child)
    }
    .await;
    let child = match dispatch_result {
        Ok(child) => child,
        Err(error) => {
            let _ = devrail_workspaces::cleanup_materialized_workspace(
                &supervisor.workspace_root(),
                std::path::Path::new(&environment.workspace_root),
                &relative,
            )
            .await;
            return Err(error);
        }
    };
    supervisor
        .launch_reserved(
            RunLaunch {
                run_id: child.id,
                task_id: task.id,
                organization_id: request.organization_id,
                department_id: request.department_id,
                owner_user_id: request.owner_user_id,
                cwd: materialized.path,
                input: format!(
                    "请根据失败诊断执行第 {} 次受控修复",
                    request.repair_sequence
                ),
                resume_thread_id: None,
                resume_turn_id: None,
                attempt: child.attempt,
                max_attempts: child.attempt.saturating_add(1),
                automatic: true,
                scheduler_policy: supervisor.scheduler_policy(),
            },
            reservation,
        )
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))
}

async fn reconcile_dispatched_continuations(pool: &PgPool, supervisor: &HarnessSupervisor) {
    let requests =
        match repositories::devrail_continuations::list_dispatched_unstarted(pool, 100).await {
            Ok(requests) => requests,
            Err(error) => {
                tracing::warn!(error = %error, "continuation dispatch reconciliation failed");
                crate::app_metrics::record_reconciliation("continuation_dispatch_reconcile_failed");
                return;
            }
        };
    for request in requests {
        let Some(child_run_id) = request.child_run_id else {
            continue;
        };
        if supervisor.running_run_ids().await.contains(&child_run_id) {
            continue;
        }
        let Some(child) = repositories::devrail_runs::find_for_recovery(pool, child_run_id)
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        let Some(source) =
            repositories::devrail_runs::find_for_recovery(pool, request.source_run_id)
                .await
                .ok()
                .flatten()
        else {
            continue;
        };
        let launch = RunLaunch {
            run_id: child.id,
            task_id: child.task_id,
            organization_id: child.organization_id,
            department_id: child.department_id,
            owner_user_id: child.owner_user_id,
            cwd: std::path::PathBuf::from(&child.cwd),
            input: request.redacted_context,
            resume_thread_id: source.thread_id,
            resume_turn_id: source.turn_id,
            attempt: child.attempt,
            max_attempts: child.attempt.saturating_add(1),
            automatic: true,
            scheduler_policy: supervisor.scheduler_policy(),
        };
        if let Err(error) = supervisor.launch(launch).await {
            tracing::warn!(child_run_id, error = %error, "continuation child launch reconciliation failed");
            crate::app_metrics::record_reconciliation("continuation_launch_failed");
        } else {
            crate::app_metrics::record_reconciliation("continuation_launch_recovered");
        }
    }
}

async fn dispatch_continuations(pool: &PgPool, supervisor: &HarnessSupervisor) {
    let worker_id = format!("scheduler:{}", Uuid::new_v4().simple());
    let claim_token = Uuid::new_v4();
    let requests = match repositories::devrail_continuations::claim_pending(
        pool,
        &worker_id,
        claim_token,
        CLAIM_BATCH_SIZE,
        supervisor.scheduler_policy().claim_lease_seconds,
    )
    .await
    {
        Ok(requests) => requests,
        Err(error) => {
            tracing::warn!(error = %error, "continuation claim failed");
            crate::app_metrics::record_scheduler_dispatch("continuation_claim_failed");
            return;
        }
    };
    for request in requests {
        let request_id = request.id;
        let dispatch_attempts = request.dispatch_attempts;
        let dispatch_started = request.created_at;
        let trigger_type = request.trigger_type.clone();
        crate::app_metrics::record_continuation_event("claimed", "claimed", &trigger_type);
        let rejection_actor = scheduler_actor_for_request(&request);
        if let Err(error) =
            dispatch_continuation(pool, supervisor, &worker_id, claim_token, request).await
        {
            tracing::warn!(error = %error, "continuation dispatch failed");
            if continuation_dispatch_is_deterministic(&error) {
                if let Ok(mut tx) = pool.begin().await {
                    if repositories::devrail_continuations::reject_claim(
                        &mut tx,
                        &rejection_actor,
                        request_id,
                        &worker_id,
                        claim_token,
                        continuation_dispatch_reason(&error),
                    )
                    .await
                    .is_ok()
                    {
                        let _ = tx.commit().await;
                        crate::app_metrics::record_continuation_event(
                            "rejected",
                            "rejected",
                            &trigger_type,
                        );
                    }
                }
            } else {
                let backoff_seconds =
                    retry_delay(dispatch_attempts, supervisor.scheduler_policy()).num_seconds();
                let _ = repositories::devrail_continuations::release_claim(
                    pool,
                    request_id,
                    &worker_id,
                    claim_token,
                    backoff_seconds,
                )
                .await;
                crate::app_metrics::record_continuation_event(
                    "recovered",
                    "pending",
                    &trigger_type,
                );
            }
            crate::app_metrics::record_scheduler_dispatch("continuation_failed");
        } else {
            crate::app_metrics::record_continuation_event(
                "dispatched",
                "dispatched",
                &trigger_type,
            );
            crate::app_metrics::record_continuation_dispatch_latency(
                Utc::now()
                    .signed_duration_since(dispatch_started)
                    .num_milliseconds()
                    .max(0) as f64
                    / 1000.0,
            );
        }
    }
}

fn continuation_dispatch_is_deterministic(error: &ApiError) -> bool {
    match error {
        ApiError::NotFound(_) | ApiError::Validation(_) | ApiError::Forbidden(_) => true,
        ApiError::Conflict(message) => {
            message.contains("不存在")
                || message.contains("不匹配")
                || message.contains("缺少")
                || message.contains("已禁用")
                || message.contains("不允许")
                || message.contains("已过期")
                || message.contains("超过")
        }
        _ => false,
    }
}

fn continuation_dispatch_reason(error: &ApiError) -> &'static str {
    match error {
        ApiError::Unavailable => "dispatch_unavailable",
        ApiError::NotFound(_) => "source_missing",
        ApiError::Validation(_) => "validation_rejected",
        ApiError::Forbidden(_) => "policy_rejected",
        ApiError::Conflict(message) if message.contains("摘要") => "evidence_mismatch",
        ApiError::Conflict(message) if message.contains("过期") => "evidence_expired",
        ApiError::Conflict(message) if message.contains("次数") => "dispatch_attempt_limit",
        ApiError::Conflict(message) if message.contains("缺少") => "evidence_missing",
        ApiError::Conflict(message) if message.contains("已禁用") => "environment_disabled",
        ApiError::Conflict(_) => "dispatch_prerequisite_invalid",
        _ => "dispatch_prerequisite_invalid",
    }
}

async fn dispatch_continuation(
    pool: &PgPool,
    supervisor: &HarnessSupervisor,
    worker_id: &str,
    claim_token: Uuid,
    request: crate::models::DevRailContinuationRequestRow,
) -> Result<(), ApiError> {
    if request
        .evidence_expires_at
        .is_some_and(|expires_at| expires_at <= Utc::now())
    {
        return Err(ApiError::conflict("continuation 触发证据已过期"));
    }
    let policy = serde_json::from_value::<ContinuationPolicy>(request.policy_snapshot.clone())
        .map_err(|_| ApiError::conflict("continuation 固化策略不可用"))?;
    let trigger = match request.trigger_type.as_str() {
        "user_context" => DevRailContinuationTrigger::UserContext,
        "quality_gate" => DevRailContinuationTrigger::QualityGate,
        "review_changes" => DevRailContinuationTrigger::ReviewChanges,
        _ => return Err(ApiError::conflict("continuation 触发类型不允许")),
    };
    if !policy.enabled || !policy.allowed_triggers.contains(&trigger) {
        return Err(ApiError::conflict("continuation 固化策略不允许派发"));
    }
    if request.dispatch_attempts > policy.max_dispatch_attempts {
        return Err(ApiError::conflict("continuation 派发次数超过固化策略"));
    }
    let source = repositories::devrail_runs::find_for_recovery(pool, request.source_run_id)
        .await
        .map_err(crate::error::db_error)?
        .ok_or_else(|| ApiError::not_found("来源运行不存在"))?;
    if !matches!(source.status.as_str(), "completed" | "failed")
        || source.root_run_id != Some(request.root_run_id)
        || source.turn_id.as_deref() != Some(request.source_turn_id.as_str())
    {
        return Err(ApiError::conflict("continuation 来源状态或谱系不匹配"));
    }
    if source.thread_id.is_none() || source.turn_id.is_none() {
        let actor = scheduler_actor_for_request(&request);
        let mut tx = pool.begin().await.map_err(crate::error::db_error)?;
        repositories::devrail_continuations::reject_claim(
            &mut tx,
            &actor,
            request.id,
            worker_id,
            claim_token,
            "source_thread_missing",
        )
        .await
        .map_err(crate::error::db_error)?;
        tx.commit().await.map_err(crate::error::db_error)?;
        return Err(ApiError::conflict("来源运行缺少可恢复 thread"));
    }
    let handoff =
        repositories::devrail_continuations::find_handoff_by_request(pool, request.id, claim_token)
            .await
            .map_err(crate::error::db_error)?
            .ok_or_else(|| ApiError::conflict("来源运行缺少有效 handoff"))?;
    let task = repositories::devrail::find_task_by_id(
        pool,
        &scheduler_actor_for_request(&request),
        request.task_id,
    )
    .await
    .map_err(crate::error::db_error)?
    .ok_or_else(|| ApiError::not_found("任务不存在"))?;
    if task.status != "continuation_pending" || task.project_id != request.project_id {
        return Err(ApiError::conflict("continuation 任务状态或范围不匹配"));
    }
    if task.repository_id != Some(handoff.repository_id) {
        return Err(ApiError::conflict("handoff 仓库身份与任务不一致"));
    }
    let repository = repositories::devrail::find_repository(
        pool,
        &scheduler_actor_for_request(&request),
        request.project_id,
        handoff.repository_id,
    )
    .await
    .map_err(crate::error::db_error)?
    .ok_or_else(|| ApiError::conflict("handoff 仓库不存在或不可用"))?;
    let environment_id = handoff
        .environment_id
        .ok_or_else(|| ApiError::conflict("handoff 缺少运行环境"))?;
    if task.environment_id != Some(environment_id) {
        return Err(ApiError::conflict("handoff 运行环境与任务不一致"));
    }
    let environment = repositories::devrail::find_environment(
        pool,
        &scheduler_actor_for_request(&request),
        request.project_id,
        environment_id,
    )
    .await
    .map_err(crate::error::db_error)?
    .ok_or_else(|| ApiError::conflict("handoff 运行环境不存在或不可用"))?;
    if !environment.enabled {
        return Err(ApiError::conflict("handoff 运行环境已禁用"));
    }
    let reservation = supervisor
        .reserve()
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let relative = devrail_workspaces::continuation_workspace_key(
        request.task_id,
        request.continuation_sequence,
    )?;
    let workspace_root = supervisor.workspace_root();
    let source_repository = std::path::Path::new(&environment.workspace_root);
    let materialized =
        devrail_workspaces::materialize_from_handoff(&devrail_workspaces::HandoffMaterialization {
            root: &workspace_root,
            source_repository,
            relative: &relative,
            repository_identity: &handoff.repository_identity,
            repository_identity_digest: &handoff.repository_identity_digest,
            repository_remote_url: &repository.remote_url,
            base_commit: &handoff.base_commit,
            changeset_ref: handoff
                .changeset_ref
                .as_deref()
                .ok_or_else(|| ApiError::conflict("handoff 缺少变更引用"))?,
            changeset_digest: &handoff.changeset_digest,
        })
        .await?;
    let actor = scheduler_actor_for_request(&request);
    let child = match persist_continuation_dispatch(ContinuationDispatchContext {
        pool,
        actor: &actor,
        request: &request,
        source: &source,
        task: &task,
        handoff: &handoff,
        worker_id,
        claim_token,
        relative: &relative,
        cwd: &materialized.path,
    })
    .await
    {
        Ok(child) => child,
        Err(error) => {
            let _ = devrail_workspaces::cleanup_handoff_workspace(
                &workspace_root,
                source_repository,
                &relative,
            )
            .await;
            return Err(error);
        }
    };
    let launch = RunLaunch {
        run_id: child.id,
        task_id: request.task_id,
        organization_id: request.organization_id,
        department_id: request.department_id,
        owner_user_id: request.owner_user_id,
        cwd: materialized.path,
        input: request.redacted_context,
        resume_thread_id: source.thread_id,
        resume_turn_id: source.turn_id,
        attempt: child.attempt,
        max_attempts: task.scheduler_max_attempts,
        automatic: true,
        scheduler_policy: supervisor.scheduler_policy(),
    };
    supervisor
        .launch_reserved(launch, reservation)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(())
}

struct ContinuationDispatchContext<'a> {
    pool: &'a PgPool,
    actor: &'a ActorContext,
    request: &'a crate::models::DevRailContinuationRequestRow,
    source: &'a crate::models::DevRailRunRow,
    task: &'a DevRailTaskRow,
    handoff: &'a crate::models::DevRailRunHandoffRow,
    worker_id: &'a str,
    claim_token: Uuid,
    relative: &'a str,
    cwd: &'a std::path::Path,
}

async fn persist_continuation_dispatch(
    context: ContinuationDispatchContext<'_>,
) -> Result<crate::models::DevRailRunRow, ApiError> {
    let mut tx = context.pool.begin().await.map_err(crate::error::db_error)?;
    let child = repositories::devrail_runs::create_continuation_run(
        &mut tx,
        &repositories::devrail_runs::NewContinuationRun {
            actor: context.actor,
            task_id: context.request.task_id,
            snapshot_id: context.source.snapshot_id,
            idempotency_key: &format!("continuation:{}", context.request.id),
            task_revision: context.task.revision,
            workflow_source: &context.source.workflow_source,
            workflow_version: &context.source.workflow_version,
            workflow_digest: &context.source.workflow_digest,
            workflow_snapshot: &context.source.workflow_snapshot,
            parent_run_id: context.request.source_run_id,
            parent_turn_id: &context.request.source_turn_id,
            thread_id: context.source.thread_id.as_deref().unwrap_or_default(),
            continuation_request_id: context.request.id,
            continuation_sequence: context.request.continuation_sequence,
            harness_start_key: &format!("continuation:{}:start", context.request.id),
            cwd: context.cwd.to_string_lossy().as_ref(),
            policy: &context.request.policy_snapshot,
            startup_args: &serde_json::json!(["app-server"]),
            model_id: context.source.model_id.as_deref(),
            department_id: context.request.department_id,
        },
    )
    .await
    .map_err(crate::error::db_error)?
    .ok_or_else(|| ApiError::conflict("continuation child run 已由其他 worker 创建"))?;
    let path_digest = devrail_workspaces::path_digest(context.relative);
    let workspace = repositories::devrail_workspaces::create(
        &mut tx,
        &repositories::devrail_workspaces::NewWorkspace {
            actor: context.actor,
            task_id: context.request.task_id,
            run_id: Some(child.id),
            attempt: child.attempt,
            workspace_key: context.relative,
            relative_path: context.relative,
            path_digest: &path_digest,
            repository_id: Some(context.handoff.repository_id),
            environment_id: context.handoff.environment_id,
            base_commit: Some(&context.handoff.base_commit),
            branch_name: context.handoff.branch_ref.as_deref(),
            workflow_version: Some(&context.source.workflow_version),
            workflow_digest: Some(&context.source.workflow_digest),
            environment_version: None,
            tool_versions: &context.handoff.tool_versions,
            snapshot_digest: Some(&context.request.input_digest),
        },
    )
    .await
    .map_err(crate::error::db_error)?
    .ok_or_else(|| ApiError::conflict("continuation workspace 已被其他请求占用"))?;
    repositories::devrail_workspaces::set_lifecycle(
        &mut tx,
        workspace.id,
        "running",
        "pending",
        Some("before_run"),
        None,
    )
    .await
    .map_err(crate::error::db_error)?;
    repositories::devrail_continuations::mark_dispatched(
        &mut tx,
        context.actor,
        context.request.id,
        context.worker_id,
        context.claim_token,
        child.id,
    )
    .await
    .map_err(crate::error::db_error)?
    .ok_or_else(|| ApiError::conflict("continuation claim 已失效"))?;
    tx.commit().await.map_err(crate::error::db_error)?;
    Ok(child)
}

fn scheduler_actor_for_request(
    request: &crate::models::DevRailContinuationRequestRow,
) -> ActorContext {
    ActorContext {
        actor_type: ActorType::System,
        user_id: request.owner_user_id,
        session_id: 0,
        organization_id: request.organization_id,
        department_id: request.department_id,
        data_scope: DataScope::All,
        permission_codes: BTreeSet::new(),
    }
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
    matches!(
        error,
        ApiError::Internal(_) | ApiError::Unavailable | ApiError::Conflict(_)
    )
}

fn dispatch_failure_reason(error: &ApiError) -> &'static str {
    match error {
        ApiError::Unauthorized | ApiError::Forbidden(_) => "调度权限或安全策略不允许执行",
        ApiError::NotFound(_) => "调度依赖资源不存在",
        ApiError::Validation(_) => "任务配置校验失败",
        ApiError::Conflict(_) => "Harness 或任务状态暂时冲突",
        ApiError::Unavailable => "Harness 或数据库暂时不可用",
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
            "devrail:task_dependency:read".to_string(),
            "devrail:task_dependency:write".to_string(),
            "devrail:followup:create".to_string(),
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
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct EmptyTracker {
        reconciliations: AtomicUsize,
        claims: AtomicUsize,
    }

    #[async_trait]
    impl TaskTracker for EmptyTracker {
        async fn find_task(
            &self,
            _actor: &ActorContext,
            _task_id: i64,
        ) -> Result<Option<DevRailTaskRow>, TrackerError> {
            Ok(None)
        }

        async fn claim_dispatch_candidates(
            &self,
            _claim_token: Uuid,
            _limit: i64,
            _claim_lease_seconds: i64,
            _priority_aging_seconds: i64,
        ) -> Result<Vec<DevRailTaskRow>, TrackerError> {
            self.claims.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }

        async fn renew_claim(
            &self,
            _task_id: i64,
            _claim_token: Uuid,
            _claim_lease_seconds: i64,
        ) -> Result<bool, TrackerError> {
            Ok(false)
        }

        async fn release_claim(
            &self,
            _task_id: i64,
            _claim_token: Uuid,
        ) -> Result<bool, TrackerError> {
            Ok(false)
        }

        async fn schedule_retry(
            &self,
            _task_id: i64,
            _claim_token: Uuid,
            _retry_at: chrono::DateTime<Utc>,
            _reason: &str,
        ) -> Result<bool, TrackerError> {
            Ok(false)
        }

        async fn fail_task(
            &self,
            _task_id: i64,
            _claim_token: Uuid,
            _reason: &str,
        ) -> Result<bool, TrackerError> {
            Ok(false)
        }

        async fn reconcile(
            &self,
            _running_run_ids: &[i64],
            _stale_timeout_seconds: i64,
        ) -> Result<crate::repositories::devrail::SchedulerReconciliation, TrackerError> {
            self.reconciliations.fetch_add(1, Ordering::SeqCst);
            Ok(crate::repositories::devrail::SchedulerReconciliation::default())
        }

        async fn reconcile_dependencies(&self) -> Result<u64, TrackerError> {
            Ok(0)
        }
    }

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
            revision: 1,
            dispatch_snapshot: json!({"schemaVersion": 1}),
            dispatch_snapshot_digest: "0".repeat(64),
            workflow_source: "legacy".to_string(),
            workflow_version: "legacy-v1".to_string(),
            workflow_digest: "0".repeat(64),
            scheduler_attempt: 0,
            scheduler_retry_count: 0,
            scheduler_max_attempts: 3,
            scheduler_retry_at: None,
            scheduler_last_error: None,
            hook_failure_fingerprint: None,
            hook_failure_count: 0,
            creation_source: "legacy".to_string(),
            source_task_id: None,
            source_run_id: None,
            followup_depth: 0,
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

    #[test]
    fn dispatch_prioritizes_continuations_before_queued_tasks() {
        assert_eq!(
            DISPATCH_PHASES,
            [
                DispatchPhase::Continuations,
                DispatchPhase::DispatchedContinuationRecovery,
                DispatchPhase::Repairs,
                DispatchPhase::DispatchedRepairRecovery,
                DispatchPhase::RepairGateReruns,
                DispatchPhase::QueuedTasks,
            ]
        );
    }

    #[test]
    fn continuation_dispatch_errors_have_stable_retry_or_rejection_classes() {
        let expired = ApiError::conflict("continuation 触发证据已过期");
        assert!(continuation_dispatch_is_deterministic(&expired));
        assert_eq!(continuation_dispatch_reason(&expired), "evidence_expired");

        let missing = ApiError::conflict("来源运行缺少有效 handoff");
        assert!(continuation_dispatch_is_deterministic(&missing));
        assert_eq!(continuation_dispatch_reason(&missing), "evidence_missing");

        let capacity = ApiError::conflict("Harness 并发额度已用尽");
        assert!(!continuation_dispatch_is_deterministic(&capacity));
    }

    #[test]
    fn repair_dispatch_keeps_capacity_transient_but_hands_off_policy_rejections() {
        let capacity = ApiError::conflict("Harness 并发额度已用尽");
        assert!(!repair_dispatch_is_deterministic(&capacity));

        let policy = ApiError::conflict("repair 固化策略已禁用");
        assert!(repair_dispatch_is_deterministic(&policy));
        assert_eq!(
            repair_dispatch_reason(&policy),
            "dispatch_prerequisite_invalid"
        );

        let hook_breaker = ApiError::conflict("Hook 失败熔断已打开");
        assert!(repair_dispatch_is_deterministic(&hook_breaker));
        assert_eq!(
            repair_dispatch_reason(&hook_breaker),
            "hook_failure_circuit_open"
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

    #[tokio::test]
    async fn scheduler_tick_uses_injected_tracker_without_database_access() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://devrail:unused@127.0.0.1:1/devrail")
            .expect("lazy test pool");
        let supervisor = HarnessSupervisor::new(
            pool.clone(),
            "codex".to_string(),
            1,
            60,
            "/tmp/devrail-workspaces".to_string(),
            1,
            SchedulerPolicy::default(),
        );
        let tracker = EmptyTracker::default();
        run_tick(
            &pool,
            &tracker,
            &supervisor,
            SchedulerPolicy::default(),
            false,
        )
        .await
        .expect("empty tracker tick");
        assert_eq!(tracker.reconciliations.load(Ordering::SeqCst), 1);
        assert_eq!(tracker.claims.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn controlled_repair_fake_app_server_workspace_and_gate_e2e() {
        const REPAIR_E2E_TIMEOUT_SECS: u64 = 30;
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture =
            crate::repositories::devrail_repairs::integration_tests::failed_fixture(&pool).await;
        let root = std::env::temp_dir().join(format!("devrail-repair-e2e-{}", Uuid::new_v4()));
        let repository = root.join("repository");
        let source_workspace = root.join("source-run");
        tokio::fs::create_dir_all(&repository)
            .await
            .expect("create repository");

        async fn git(repository: &std::path::Path, args: &[&str]) {
            let output = tokio::process::Command::new("git")
                .arg("-C")
                .arg(repository)
                .args(args)
                .env_clear()
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .await
                .expect("run git");
            assert!(
                output.status.success(),
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        git(&repository, &["init"]).await;
        git(
            &repository,
            &["config", "user.email", "devrail@example.test"],
        )
        .await;
        git(&repository, &["config", "user.name", "DevRail Test"]).await;
        tokio::fs::write(repository.join("tracked.txt"), "initial\n")
            .await
            .expect("write tracked file");
        tokio::fs::write(
            repository.join("package.json"),
            r#"{"scripts":{"test:ci":"node -e \"process.exit(0)\""}}"#,
        )
        .await
        .expect("write package manifest");
        tokio::fs::write(
            repository.join("app-server"),
            r#"IFS= read -r initialize
printf '%s\n' '{"id":"initialize","result":{}}'
IFS= read -r thread_request
IFS= read -r turn_request
printf '%s\n' '{"type":"agent_message","id":"sensitive-event","message":"repair complete","token":"FAKE_TOKEN","authorization":"Bearer FAKE_AUTH","command":"npm run test:ci","cwd":"/absolute/secret/workspace","path":"/absolute/secret/file"}'
printf '%s\n' '{"type":"agent_message","id":"repair-thread-event","message":"workspace updated","thread_id":"repair-thread","turn_id":"repair-turn"}'
printf '%s\n' '{"type":"turn_complete","id":"repair-done","message":"完成"}'
printf '%s\n' 'repaired' > tracked.txt
exit 0
"#,
        )
        .await
        .expect("write fake app-server");
        git(&repository, &["add", "."]).await;
        git(&repository, &["commit", "-m", "initial"]).await;
        git(
            &repository,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/devrail.git",
            ],
        )
        .await;
        let base_commit = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["rev-parse", "HEAD"])
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .await
            .expect("read base commit");
        let base_commit = String::from_utf8_lossy(&base_commit.stdout)
            .trim()
            .to_string();
        git(
            &repository,
            &[
                "worktree",
                "add",
                "--detach",
                source_workspace.to_string_lossy().as_ref(),
                &base_commit,
            ],
        )
        .await;
        tokio::fs::write(source_workspace.join("tracked.txt"), "source change\n")
            .await
            .expect("write source change");

        let repair_policy = serde_json::to_value(RepairPolicy {
            enabled: true,
            ..RepairPolicy::default()
        })
        .expect("serialize repair policy");
        crate::repositories::devrail_repairs::integration_tests::configure_controlled_repair_fixture(
            &pool,
            &fixture,
            repository.to_string_lossy().as_ref(),
            &serde_json::from_value(repair_policy).expect("decode repair policy"),
        )
        .await;
        let source_digest = crate::services::devrail_workspaces::path_digest("source-run");
        let source_actor = ActorContext {
            actor_type: ActorType::System,
            user_id: fixture.actor.user_id,
            session_id: 0,
            organization_id: fixture.actor.organization_id,
            department_id: fixture.actor.department_id,
            data_scope: DataScope::All,
            permission_codes: BTreeSet::new(),
        };
        let mut workspace_tx = pool.begin().await.expect("begin source workspace");
        crate::repositories::devrail_workspaces::create(
            &mut workspace_tx,
            &crate::repositories::devrail_workspaces::NewWorkspace {
                actor: &source_actor,
                task_id: fixture.task_id,
                run_id: Some(fixture.source_run_id),
                attempt: 1,
                workspace_key: "source-run",
                relative_path: "source-run",
                path_digest: &source_digest,
                repository_id: Some(fixture.repository_id),
                environment_id: Some(fixture.environment_id),
                base_commit: Some(&base_commit),
                branch_name: None,
                workflow_version: Some("legacy-v1"),
                workflow_digest: Some(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                ),
                environment_version: None,
                tool_versions: &json!({"git":"test"}),
                snapshot_digest: Some(&fixture.source_run_id.to_string()),
            },
        )
        .await
        .expect("persist source workspace");
        workspace_tx
            .commit()
            .await
            .expect("commit source workspace");
        let handoff = crate::services::devrail_workspaces::capture_handoff_evidence(
            &root,
            fixture.source_run_id,
            "source-run",
            Some(&base_commit),
        )
        .await
        .expect("capture source changeset");
        crate::services::devrail_continuations::persist_handoff(
            &pool,
            &source_actor,
            fixture.source_run_id,
            &root,
        )
        .await
        .expect("persist source handoff");
        let mut event_tx = pool.begin().await.expect("begin failed gate event");
        crate::repositories::devrail_runs::append_event(
            &mut event_tx,
            &crate::repositories::devrail_runs::NewRunEvent {
                run_id: fixture.source_run_id,
                organization_id: fixture.actor.organization_id,
                department_id: fixture.actor.department_id,
                owner_user_id: fixture.actor.user_id,
                event_type: "quality_gate",
                source_event_id: Some("repair-e2e-gate"),
                idempotency_key: "repair-e2e-gate",
                payload: &json!({"name":"backend_tests","status":"failed","log_ref":"quality-gates/backend-tests"}),
                summary: Some("质量门禁未通过"),
            },
        )
        .await
        .expect("persist failed gate");
        event_tx.commit().await.expect("commit failed gate");
        assert_eq!(handoff.changeset_digest.len(), 64);
        let request = devrail_repairs::create_for_failed_quality_gates(
            &pool,
            &fixture.actor,
            fixture.source_run_id,
            &crate::models::CreateDevRailRepairRequest {
                idempotency_key: "repair-e2e-request".to_string(),
                risk_category: crate::models::DevRailRepairRiskCategory::LowRisk,
            },
        )
        .await
        .expect("create repair request");
        assert_eq!(request.status, "pending");
        let claim_token = Uuid::new_v4();
        let claimed = crate::repositories::devrail_repairs::claim_pending(
            &pool,
            "repair-e2e-worker",
            claim_token,
            10,
            60,
        )
        .await
        .expect("claim repair request");
        let claimed_request = claimed
            .into_iter()
            .find(|row| row.id == request.id)
            .expect("claimed request");
        let supervisor = HarnessSupervisor::new(
            pool.clone(),
            "bash".to_string(),
            1,
            REPAIR_E2E_TIMEOUT_SECS as i64,
            root.to_string_lossy().into_owned(),
            1,
            SchedulerPolicy::default(),
        );
        dispatch_repair(
            &pool,
            &supervisor,
            "repair-e2e-worker",
            claim_token,
            claimed_request,
        )
        .await
        .expect("dispatch repair child");
        let child_id: i64 =
            sqlx::query_scalar("SELECT child_run_id FROM devrail_repair_requests WHERE id=$1")
                .bind(request.id)
                .fetch_one(&pool)
                .await
                .expect("read child run id");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(REPAIR_E2E_TIMEOUT_SECS);
        loop {
            let status: String = sqlx::query_scalar("SELECT status FROM devrail_runs WHERE id=$1")
                .bind(child_id)
                .fetch_one(&pool)
                .await
                .expect("read child status");
            if status == "completed" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "repair child did not complete"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let child = crate::repositories::devrail_runs::find_for_recovery(&pool, child_id)
            .await
            .expect("read child")
            .expect("child exists");
        assert_eq!(child.run_kind, "repair");
        assert_eq!(child.parent_run_id, Some(fixture.source_run_id));
        assert!(child.cwd.starts_with(root.to_string_lossy().as_ref()));
        let source_status: String =
            sqlx::query_scalar("SELECT status FROM devrail_runs WHERE id=$1")
                .bind(fixture.source_run_id)
                .fetch_one(&pool)
                .await
                .expect("read source status");
        assert_eq!(source_status, "failed");
        let gate_count_before: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM devrail_repair_gate_reruns WHERE repair_request_id=$1",
        )
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("count gate reruns");
        assert_eq!(gate_count_before, 1);
        dispatch_repair_gate_reruns(&pool, &supervisor).await;
        let request_deadline =
            tokio::time::Instant::now() + Duration::from_secs(REPAIR_E2E_TIMEOUT_SECS);
        loop {
            let status: String =
                sqlx::query_scalar("SELECT status FROM devrail_repair_requests WHERE id=$1")
                    .bind(request.id)
                    .fetch_one(&pool)
                    .await
                    .expect("read repair status");
            if status == "succeeded" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < request_deadline,
                "repair request did not finalize"
            );
            dispatch_repair_gate_reruns(&pool, &supervisor).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let task_status: String =
            sqlx::query_scalar("SELECT status FROM devrail_tasks WHERE id=$1")
                .bind(fixture.task_id)
                .fetch_one(&pool)
                .await
                .expect("read repaired task");
        assert_eq!(task_status, "succeeded");
        let gate_count_after: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM devrail_repair_gate_reruns WHERE repair_request_id=$1",
        )
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("count replayed gates");
        assert_eq!(gate_count_after, 1);
        let child_event_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM devrail_run_events WHERE run_id=$1 AND event_type='turn_complete' AND idempotency_key LIKE 'terminal:%'",
        )
        .bind(child_id)
        .fetch_one(&pool)
        .await
        .expect("count terminal events");
        assert_eq!(child_event_count, 1);
        let mut replay_tx = pool.begin().await.expect("begin terminal replay");
        assert!(!crate::repositories::devrail_runs::update_run_terminal(
            &mut replay_tx,
            &crate::repositories::devrail_runs::TerminalRunUpdate {
                run_id: child_id,
                status: "completed",
                exit_reason: "completed",
                exit_code: Some(0),
                stderr_summary: None,
                trace_id: "repair-e2e-replay",
                recovery_suggestion: None,
            },
        )
        .await
        .expect("replay terminal event"));
        replay_tx.commit().await.expect("commit terminal replay");
        for query in [
            "SELECT COALESCE(string_agg(payload::text || COALESCE(summary,''), ' '),'') FROM devrail_run_events WHERE run_id=$1",
            "SELECT COALESCE(string_agg(error_summary || structured_error::text || environment_summary::text, ' '),'') FROM devrail_repair_diagnoses WHERE source_run_id=$1",
            "SELECT COALESCE(string_agg(details::text, ' '),'') FROM audit_logs WHERE target_id=$1",
            "SELECT COALESCE(string_agg(summary, ' '),'') FROM devrail_notifications WHERE resource_id=$1",
            "SELECT COALESCE(string_agg(payload::text, ' '),'') FROM devrail_outbox_events WHERE aggregate_id=$1",
        ] {
            let bind_id = if query.contains("diagnoses") {
                fixture.source_run_id
            } else if query.contains("events WHERE run_id") {
                child_id
            } else {
                request.id
            };
            let text: String = sqlx::query_scalar(query)
                .bind(bind_id)
                .fetch_one(&pool)
                .await
                .expect("read redacted evidence");
            for secret in ["FAKE_TOKEN", "FAKE_AUTH", "npm run test:ci", "/absolute/secret"] {
                assert!(!text.contains(secret), "sensitive value leaked: {secret}");
            }
        }
        let relative = std::path::Path::new(&child.cwd)
            .strip_prefix(&root)
            .expect("child path under root")
            .to_string_lossy()
            .into_owned();
        devrail_workspaces::cleanup_materialized_workspace(&root, &repository, &relative)
            .await
            .expect("cleanup repair workspace");
        devrail_workspaces::cleanup_materialized_workspace(&root, &repository, &relative)
            .await
            .expect("cleanup replay");
        let rebuilt =
            devrail_workspaces::materialize_repair_from_source(&root, &repository, &relative, None)
                .await
                .expect("rebuild repair workspace");
        assert!(rebuilt.path.starts_with(&root));
        devrail_workspaces::cleanup_materialized_workspace(&root, &repository, &relative)
            .await
            .expect("cleanup rebuilt workspace");
        tokio::fs::remove_dir_all(&root)
            .await
            .expect("cleanup repair e2e root");
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup repair E2E schema");
    }
}
