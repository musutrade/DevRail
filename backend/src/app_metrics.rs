//! Low-cardinality Prometheus metrics for HTTP traffic and the PostgreSQL pool.

use axum::extract::{MatchedPath, Request, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::{LazyLock, Once};
use std::time::Instant;

use crate::AppState;

static PROMETHEUS: LazyLock<PrometheusHandle> = LazyLock::new(|| {
    PrometheusBuilder::new()
        .with_recommended_naming(true)
        .set_buckets(&[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0])
        .expect("valid HTTP latency buckets")
        .install_recorder()
        .expect("install Prometheus metrics recorder")
});
static DESCRIBE_METRICS: Once = Once::new();

pub fn initialize() {
    LazyLock::force(&PROMETHEUS);
    DESCRIBE_METRICS.call_once(|| {
        describe_counter!("arc_admin_http_requests", "HTTP 请求总数");
        describe_histogram!(
            "arc_admin_http_request_duration_seconds",
            "HTTP 请求耗时（秒）"
        );
        describe_gauge!("arc_admin_db_pool_size", "PostgreSQL 连接池当前连接数");
        describe_gauge!("arc_admin_db_pool_idle", "PostgreSQL 连接池空闲连接数");
        describe_gauge!("arc_admin_db_pool_acquired", "PostgreSQL 连接池占用连接数");
        describe_counter!(
            "devrail_push_delivery_total",
            "DevRail Web Push 投递结果总数"
        );
        describe_gauge!("devrail_push_delivery_backlog", "待处理 Web Push 投递数量");
        describe_gauge!(
            "devrail_push_invalid_devices",
            "永久失败的 Web Push 设备数量"
        );
        describe_gauge!("devrail_scheduler_queue_depth", "DevRail 调度队列深度");
        describe_counter!(
            "devrail_scheduler_dispatch_total",
            "DevRail 调度派发结果总数"
        );
        describe_counter!(
            "devrail_scheduler_claim_conflict_total",
            "DevRail 调度 claim 冲突总数"
        );
        describe_histogram!(
            "devrail_scheduler_dispatch_latency_seconds",
            "DevRail 任务入队到派发延迟（秒）"
        );
        describe_counter!("devrail_scheduler_retry_total", "DevRail 调度重试总数");
        describe_counter!("devrail_scheduler_stall_total", "DevRail 调度 stall 总数");
        describe_gauge!("devrail_run_active", "DevRail 活动运行数");
        describe_counter!(
            "devrail_run_reconciliation_total",
            "DevRail 运行对账修正总数"
        );
        describe_counter!(
            "devrail_task_dependency_propagation_total",
            "DevRail 任务依赖终态传播总数"
        );
        describe_counter!(
            "devrail_task_dependency_conflict_total",
            "DevRail 任务依赖写入冲突总数"
        );
        describe_histogram!(
            "devrail_task_dependency_query_duration_seconds",
            "DevRail 任务依赖查询耗时（秒）"
        );
        describe_counter!(
            "devrail_agent_followup_total",
            "DevRail Agent 后续任务提议结果总数"
        );
        describe_counter!(
            "devrail_workflow_reload_total",
            "DevRail workflow 动态加载结果总数"
        );
        describe_histogram!(
            "devrail_workflow_reload_duration_seconds",
            "DevRail workflow 对账耗时（秒）"
        );
        describe_gauge!(
            "devrail_workflow_reload_healthy",
            "DevRail workflow reloader 最近一次对账是否成功"
        );
        describe_counter!(
            "devrail_workspace_event_total",
            "DevRail 任务工作区生命周期事件总数"
        );
        describe_counter!(
            "arc_admin_continuation_requests_total",
            "DevRail continuation 请求生命周期事件总数"
        );
        describe_gauge!(
            "arc_admin_continuation_pending",
            "DevRail continuation 待处理请求数量"
        );
        describe_histogram!(
            "arc_admin_continuation_dispatch_latency_seconds",
            "DevRail continuation 请求创建到派发延迟（秒）"
        );
        describe_counter!(
            "arc_admin_continuation_claim_conflict_total",
            "DevRail continuation claim 冲突总数"
        );
        describe_counter!(
            "arc_admin_continuation_replay_total",
            "DevRail continuation 幂等重放总数"
        );
        describe_counter!(
            "arc_admin_continuation_child_result_total",
            "DevRail continuation child 终态结果总数"
        );
        describe_counter!(
            "arc_admin_repair_requests_total",
            "DevRail repair 请求生命周期事件总数"
        );
        describe_counter!(
            "arc_admin_repair_diagnosis_rejected_total",
            "DevRail repair 诊断拒绝总数"
        );
        describe_counter!(
            "arc_admin_repair_claim_conflict_total",
            "DevRail repair claim 冲突总数"
        );
        describe_histogram!(
            "arc_admin_repair_dispatch_latency_seconds",
            "DevRail repair 请求创建到派发延迟（秒）"
        );
        describe_counter!(
            "arc_admin_repair_gate_rerun_total",
            "DevRail repair 门禁重跑结果总数"
        );
        describe_counter!(
            "arc_admin_repair_handoff_total",
            "DevRail repair 人工交接总数"
        );
        describe_counter!(
            "arc_admin_repair_budget_rejected_total",
            "DevRail repair 预算拒绝总数"
        );
        describe_counter!(
            "arc_admin_repair_hook_circuit_total",
            "DevRail repair Hook 熔断协同总数"
        );
        describe_counter!(
            "arc_admin_repair_child_result_total",
            "DevRail repair child 终态结果总数"
        );
    });
}

pub fn record_scheduler_dispatch(outcome: &str) {
    counter!("devrail_scheduler_dispatch_total", "outcome" => scheduler_dispatch_outcome(outcome))
        .increment(1);
}

pub fn record_scheduler_retry() {
    counter!("devrail_scheduler_retry_total").increment(1);
}

pub fn record_scheduler_claim_conflict() {
    counter!("devrail_scheduler_claim_conflict_total").increment(1);
}

pub fn record_scheduler_stall() {
    counter!("devrail_scheduler_stall_total").increment(1);
}

pub fn record_reconciliation(outcome: &str) {
    counter!("devrail_run_reconciliation_total", "outcome" => reconciliation_outcome(outcome))
        .increment(1);
}

pub fn record_dependency_propagation(outcome: &str, count: u64) {
    counter!(
        "devrail_task_dependency_propagation_total",
        "outcome" => dependency_propagation_outcome(outcome)
    )
    .increment(count);
}

pub fn record_dependency_conflict(outcome: &str) {
    counter!(
        "devrail_task_dependency_conflict_total",
        "outcome" => dependency_conflict_outcome(outcome)
    )
    .increment(1);
}

pub fn record_dependency_query_duration(seconds: f64) {
    histogram!("devrail_task_dependency_query_duration_seconds").record(seconds.max(0.0));
}

pub fn record_agent_followup(outcome: &str) {
    counter!("devrail_agent_followup_total", "outcome" => followup_outcome(outcome)).increment(1);
}

fn scheduler_dispatch_outcome(outcome: &str) -> &'static str {
    match outcome {
        "empty" => "empty",
        "stale_claim" => "stale_claim",
        "capacity" => "capacity",
        "failed" => "failed",
        "permanent_failure" => "permanent_failure",
        "started" => "started",
        _ => "other",
    }
}

fn reconciliation_outcome(outcome: &str) -> &'static str {
    match outcome {
        "released_claim" => "released_claim",
        "stale_run" => "stale_run",
        "retry_exhausted" => "retry_exhausted",
        "task_cancelled" => "task_cancelled",
        "environment_invalid" => "environment_invalid",
        "ok" => "ok",
        _ => "other",
    }
}

fn dependency_propagation_outcome(outcome: &str) -> &'static str {
    match outcome {
        "applied" => "applied",
        "noop" => "noop",
        _ => "other",
    }
}

fn dependency_conflict_outcome(outcome: &str) -> &'static str {
    match outcome {
        "cycle" => "cycle",
        "revision" => "revision",
        "idempotency" => "idempotency",
        _ => "other",
    }
}

fn followup_outcome(outcome: &str) -> &'static str {
    match outcome {
        "accepted" => "accepted",
        "replayed" => "replayed",
        "rejected_schema" => "rejected_schema",
        "rejected_policy" => "rejected_policy",
        "unavailable" => "unavailable",
        _ => "other",
    }
}

pub fn record_scheduler_dispatch_latency(seconds: f64) {
    histogram!("devrail_scheduler_dispatch_latency_seconds").record(seconds.max(0.0));
}

pub fn record_scheduler_queue_depth(depth: i64) {
    gauge!("devrail_scheduler_queue_depth").set(depth.max(0) as f64);
}

pub fn record_active_runs(count: i64) {
    gauge!("devrail_run_active").set(count.max(0) as f64);
}

pub fn record_workflow_reload(outcome: &str) {
    counter!("devrail_workflow_reload_total", "outcome" => workflow_reload_outcome(outcome))
        .increment(1);
}

pub fn record_workflow_reload_duration(duration: std::time::Duration) {
    histogram!("devrail_workflow_reload_duration_seconds").record(duration.as_secs_f64());
}

pub fn record_workflow_reload_health(healthy: bool) {
    gauge!("devrail_workflow_reload_healthy").set(if healthy { 1.0 } else { 0.0 });
}

pub fn record_workspace_event(operation: &str, outcome: &str) {
    counter!(
        "devrail_workspace_event_total",
        "operation" => workspace_operation(operation),
        "outcome" => workspace_outcome(outcome)
    )
    .increment(1);
}

pub fn record_continuation_event(event: &str, status: &str, trigger: &str) {
    counter!(
        "arc_admin_continuation_requests_total",
        "event" => continuation_event(event),
        "status" => continuation_status(status),
        "trigger" => continuation_trigger(trigger)
    )
    .increment(1);
}

pub fn record_continuation_pending(depth: i64) {
    gauge!("arc_admin_continuation_pending").set(depth.max(0) as f64);
}

pub fn record_continuation_dispatch_latency(seconds: f64) {
    histogram!("arc_admin_continuation_dispatch_latency_seconds").record(seconds.max(0.0));
}

pub fn record_continuation_claim_conflict() {
    counter!("arc_admin_continuation_claim_conflict_total").increment(1);
}

pub fn record_continuation_replay() {
    counter!("arc_admin_continuation_replay_total").increment(1);
}

pub fn record_continuation_child_result(result: &str) {
    counter!(
        "arc_admin_continuation_child_result_total",
        "result" => continuation_result(result)
    )
    .increment(1);
}

pub fn record_repair_request(event: &str, status: &str, risk: &str) {
    counter!(
        "arc_admin_repair_requests_total",
        "event" => repair_event(event),
        "status" => repair_status(status),
        "risk" => repair_risk(risk)
    )
    .increment(1);
}

pub fn record_repair_diagnosis_rejected(reason: &str) {
    counter!("arc_admin_repair_diagnosis_rejected_total", "reason" => repair_reason(reason))
        .increment(1);
}

pub fn record_repair_claim_conflict() {
    counter!("arc_admin_repair_claim_conflict_total").increment(1);
}

pub fn record_repair_dispatch_latency(seconds: f64) {
    histogram!("arc_admin_repair_dispatch_latency_seconds").record(seconds.max(0.0));
}

pub fn record_repair_gate_rerun(result: &str) {
    counter!("arc_admin_repair_gate_rerun_total", "result" => repair_result(result)).increment(1);
}

pub fn record_repair_handoff(reason: &str) {
    counter!("arc_admin_repair_handoff_total", "reason" => repair_reason(reason)).increment(1);
}

pub fn record_repair_budget_rejected() {
    counter!("arc_admin_repair_budget_rejected_total").increment(1);
}

pub fn record_repair_hook_circuit() {
    counter!("arc_admin_repair_hook_circuit_total").increment(1);
}

pub fn record_repair_child_result(result: &str) {
    counter!("arc_admin_repair_child_result_total", "result" => repair_result(result)).increment(1);
}

fn repair_event(event: &str) -> &'static str {
    match event {
        "created" => "created",
        "claimed" => "claimed",
        "dispatched" => "dispatched",
        "completed" => "completed",
        "cancelled" => "cancelled",
        "handed_off" => "handed_off",
        "replayed" => "replayed",
        _ => "other",
    }
}

fn repair_status(status: &str) -> &'static str {
    match status {
        "pending" => "pending",
        "claimed" => "claimed",
        "dispatched" => "dispatched",
        "running" => "running",
        "succeeded" => "succeeded",
        "failed" => "failed",
        "cancelled" => "cancelled",
        "handed_off" => "handed_off",
        _ => "other",
    }
}

fn repair_risk(risk: &str) -> &'static str {
    match risk {
        "low_risk" => "low_risk",
        "logical_change" => "logical_change",
        "dependency_change" => "dependency_change",
        "remote_write" => "remote_write",
        "security_change" => "security_change",
        "forbidden" => "forbidden",
        _ => "other",
    }
}

fn repair_result(result: &str) -> &'static str {
    match result {
        "succeeded" | "passed" | "completed" => "succeeded",
        "failed" | "gate_failed" => "failed",
        "cancelled" => "cancelled",
        "manual_handoff" | "handed_off" => "manual_handoff",
        _ => "other",
    }
}

fn repair_reason(reason: &str) -> &'static str {
    match reason {
        "policy_disabled" => "policy_disabled",
        "budget_exceeded" => "budget_exceeded",
        "hook_failure_circuit_open" => "hook_failure_circuit_open",
        "forbidden_operation" => "forbidden_operation",
        "evidence_expired" => "evidence_expired",
        "evidence_mismatch" => "evidence_mismatch",
        "evidence_missing" => "evidence_missing",
        "diagnostic_too_large" => "diagnostic_too_large",
        "source_missing" => "source_missing",
        "approval_required" => "approval_required",
        _ => "other",
    }
}

fn continuation_event(event: &str) -> &'static str {
    match event {
        "created" => "created",
        "claimed" => "claimed",
        "dispatched" => "dispatched",
        "cancelled" => "cancelled",
        "rejected" => "rejected",
        "completed" => "completed",
        "recovered" => "recovered",
        _ => "other",
    }
}

fn continuation_status(status: &str) -> &'static str {
    match status {
        "pending" => "pending",
        "claimed" => "claimed",
        "dispatched" => "dispatched",
        "completed" => "completed",
        "cancelled" => "cancelled",
        "rejected" => "rejected",
        _ => "other",
    }
}

fn continuation_trigger(trigger: &str) -> &'static str {
    match trigger {
        "user_context" => "user_context",
        "quality_gate" => "quality_gate",
        "review_changes" => "review_changes",
        _ => "other",
    }
}

fn continuation_result(result: &str) -> &'static str {
    match result {
        "completed" => "completed",
        "failed" => "failed",
        "cancelled" => "cancelled",
        "interrupted" => "interrupted",
        _ => "other",
    }
}

fn workspace_operation(operation: &str) -> &'static str {
    match operation {
        "create" => "create",
        "rebuild" => "rebuild",
        "cleanup" => "cleanup",
        "hook" => "hook",
        "reconcile" => "reconcile",
        _ => "other",
    }
}

fn workspace_outcome(outcome: &str) -> &'static str {
    match outcome {
        "started" => "started",
        "succeeded" => "succeeded",
        "failed" => "failed",
        "retry" => "retry",
        _ => "other",
    }
}

fn workflow_reload_outcome(outcome: &str) -> &'static str {
    match outcome {
        "accepted" => "accepted",
        "unchanged" => "unchanged",
        "rejected_with_fallback" => "rejected_with_fallback",
        "rejected_without_fallback" => "rejected_without_fallback",
        _ => "other",
    }
}

pub fn record_push_delivery(outcome: &str) {
    counter!("devrail_push_delivery_total", "outcome" => outcome.to_string()).increment(1);
}

pub fn record_push_backlog(backlog: i64) {
    gauge!("devrail_push_delivery_backlog").set(backlog.max(0) as f64);
}

pub fn record_push_invalid_devices(count: i64) {
    gauge!("devrail_push_invalid_devices").set(count.max(0) as f64);
}

pub async fn render(State(state): State<AppState>) -> (HeaderMap, String) {
    initialize();
    let size = state.pool.size();
    let idle = state.pool.num_idle().min(size as usize) as u32;
    gauge!("arc_admin_db_pool_size").set(f64::from(size));
    gauge!("arc_admin_db_pool_idle").set(f64::from(idle));
    gauge!("arc_admin_db_pool_acquired").set(f64::from(size - idle));

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    (headers, PROMETHEUS.render())
}

pub async fn record_http_request(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().as_str().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("unmatched")
        .to_string();
    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();
    let labels = [("method", method), ("route", route), ("status", status)];
    counter!("arc_admin_http_requests", &labels).increment(1);
    histogram!("arc_admin_http_request_duration_seconds", &labels)
        .record(started.elapsed().as_secs_f64());
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_metric_labels_are_low_cardinality() {
        assert_eq!(scheduler_dispatch_outcome("started"), "started");
        assert_eq!(scheduler_dispatch_outcome("task-123"), "other");
        assert_eq!(reconciliation_outcome("task_cancelled"), "task_cancelled");
        assert_eq!(reconciliation_outcome("run-456"), "other");
        assert_eq!(workflow_reload_outcome("accepted"), "accepted");
        assert_eq!(workflow_reload_outcome("environment-42"), "other");
        assert_eq!(dependency_propagation_outcome("applied"), "applied");
        assert_eq!(dependency_conflict_outcome("task-123"), "other");
        assert_eq!(followup_outcome("source-run-42"), "other");
        assert_eq!(workspace_operation("cleanup"), "cleanup");
        assert_eq!(workspace_operation("workspace-42"), "other");
        assert_eq!(workspace_outcome("retry"), "retry");
        assert_eq!(workspace_outcome("path-/tmp"), "other");
        assert_eq!(continuation_event("created"), "created");
        assert_eq!(continuation_event("request-42"), "other");
        assert_eq!(continuation_status("pending"), "pending");
        assert_eq!(continuation_trigger("quality_gate"), "quality_gate");
        assert_eq!(continuation_result("run-42"), "other");
        assert_eq!(repair_event("created"), "created");
        assert_eq!(repair_event("request-42"), "other");
        assert_eq!(repair_status("running"), "running");
        assert_eq!(repair_status("request-42"), "other");
        assert_eq!(repair_risk("low_risk"), "low_risk");
        assert_eq!(repair_risk("request-42"), "other");
        assert_eq!(repair_result("gate_failed"), "failed");
        assert_eq!(repair_result("run-42"), "other");
        assert_eq!(repair_reason("evidence_mismatch"), "evidence_mismatch");
        assert_eq!(repair_reason("request-42"), "other");
    }
}
