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
    }
}
