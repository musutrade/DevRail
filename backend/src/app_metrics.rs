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
    });
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
