//! Structured application logging and per-request correlation context.

use crate::app_metrics;
use crate::config::{AppConfig, LogFormat};
use crate::error;
use axum::body::Body;
use axum::extract::{MatchedPath, Request};
use axum::http::{HeaderName, Response};
use axum::middleware::{self, Next};
use axum::response::Response as AxumResponse;
use axum::Router;
use opentelemetry::global;
use opentelemetry::propagation::Extractor;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::any::Any;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::request_id::{
    MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};
use tower_http::trace::TraceLayer;
use tracing::field::Empty;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

tokio::task_local! {
    static TRACE_ID: String;
}

#[derive(Clone)]
pub struct TelemetryMetadata {
    service_name: String,
    environment: &'static str,
}

impl TelemetryMetadata {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            service_name: config.service_name.clone(),
            environment: config.environment.as_str(),
        }
    }

    fn for_tests() -> Self {
        Self {
            service_name: "arc-admin-backend".to_string(),
            environment: "test",
        }
    }
}

pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.tracer_provider.take() {
            let _ = provider.shutdown_with_timeout(Duration::from_secs(5));
        }
    }
}

pub fn init(format: LogFormat) -> anyhow::Result<TelemetryGuard> {
    let tracer_provider = build_tracer_provider()?;
    let tracer = tracer_provider
        .as_ref()
        .map(|provider| provider.tracer("arc-admin-backend"));
    match format {
        LogFormat::Pretty => tracing_subscriber::registry()
            .with(logging_filter())
            .with(tracer.map(|tracer| tracing_opentelemetry::layer().with_tracer(tracer)))
            .with(tracing_subscriber::fmt::layer().compact())
            .try_init()
            .map_err(|error| anyhow::anyhow!("failed to initialize logging: {error}"))?,
        LogFormat::Json => tracing_subscriber::registry()
            .with(logging_filter())
            .with(tracer.map(|tracer| tracing_opentelemetry::layer().with_tracer(tracer)))
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .flatten_event(false)
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .try_init()
            .map_err(|error| anyhow::anyhow!("failed to initialize logging: {error}"))?,
    }
    Ok(TelemetryGuard { tracer_provider })
}

fn logging_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tower_http=info"))
}

fn build_tracer_provider() -> anyhow::Result<Option<SdkTracerProvider>> {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };
    let service_name =
        std::env::var("SERVICE_NAME").unwrap_or_else(|_| "arc-admin-backend".to_string());
    let environment = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .build()?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_service_name(service_name)
                .with_attributes([
                    KeyValue::new("deployment.environment.name", environment),
                    KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                ])
                .build(),
        )
        .build();
    global::set_text_map_propagator(TraceContextPropagator::new());
    global::set_tracer_provider(provider.clone());
    Ok(Some(provider))
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let message = panic_message(info.payload());
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown".to_string());
        let trace_id = current_trace_id().unwrap_or_else(|| "unavailable".to_string());
        tracing::error!(
            event = "application.panic",
            trace_id = %trace_id,
            panic_location = %location,
            panic_message = %message,
            "application panic"
        );
    }));
}

pub fn application_span(metadata: &TelemetryMetadata) -> tracing::Span {
    tracing::info_span!(
        "application",
        service = %metadata.service_name,
        environment = metadata.environment,
        version = env!("CARGO_PKG_VERSION")
    )
}

pub fn with_http_observability(router: Router, metadata: TelemetryMetadata) -> Router {
    let request_metadata = metadata.clone();
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(move |request: &Request| {
            let trace_id = request_trace_id(request).unwrap_or("missing");
            let route = request
                .extensions()
                .get::<MatchedPath>()
                .map(MatchedPath::as_str)
                .unwrap_or("unmatched");
            tracing::info_span!(
                "http.request",
                event = "http.request",
                trace_id,
                service = %request_metadata.service_name,
                environment = request_metadata.environment,
                version = env!("CARGO_PKG_VERSION"),
                method = %request.method(),
                route,
                user_id = Empty,
                status_code = Empty,
                latency_ms = Empty,
            )
        })
        .on_request(())
        .on_response(
            |response: &Response<_>, latency: Duration, span: &tracing::Span| {
                let status = response.status();
                let status_code = u64::from(status.as_u16());
                let latency_ms = latency.as_millis().min(u128::from(u64::MAX)) as u64;
                let trace_id = response
                    .headers()
                    .get(&REQUEST_ID_HEADER)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing");
                span.record("status_code", status_code);
                span.record("latency_ms", latency_ms);
                if status.is_server_error() {
                    tracing::error!(
                        parent: span,
                        event = "http.response",
                        trace_id,
                        status_code,
                        latency_ms,
                        "HTTP request failed"
                    );
                } else if status.is_client_error() {
                    tracing::warn!(
                        parent: span,
                        event = "http.response",
                        trace_id,
                        status_code,
                        latency_ms,
                        "HTTP request rejected"
                    );
                } else {
                    tracing::info!(
                        parent: span,
                        event = "http.response",
                        trace_id,
                        status_code,
                        latency_ms,
                        "HTTP request completed"
                    );
                }
            },
        )
        .on_failure(());

    router.layer(
        ServiceBuilder::new()
            .layer(middleware::from_fn(normalize_request_id))
            .layer(SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid))
            .layer(middleware::from_fn(app_metrics::record_http_request))
            .layer(trace_layer)
            .layer(middleware::from_fn(propagate_trace_context))
            .layer(middleware::from_fn(scope_request_trace))
            .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
            .layer(CatchPanicLayer::custom(http_panic_response)),
    )
}

pub fn default_http_observability(router: Router) -> Router {
    with_http_observability(router, TelemetryMetadata::for_tests())
}

pub fn current_trace_id() -> Option<String> {
    TRACE_ID.try_with(Clone::clone).ok()
}

pub fn record_authenticated_user(user_id: i64) {
    tracing::Span::current().record("user_id", user_id);
}

async fn normalize_request_id(mut request: Request, next: Next) -> AxumResponse {
    let is_valid = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(valid_request_id);
    if request.headers().contains_key(&REQUEST_ID_HEADER) && !is_valid {
        request.headers_mut().remove(&REQUEST_ID_HEADER);
        tracing::warn!(
            event = "http.invalid_request_id",
            "ignored invalid request correlation header"
        );
    }
    next.run(request).await
}

async fn scope_request_trace(request: Request, next: Next) -> AxumResponse {
    let trace_id = request_trace_id(&request)
        .unwrap_or("unavailable")
        .to_string();
    TRACE_ID.scope(trace_id, next.run(request)).await
}

async fn propagate_trace_context(request: Request, next: Next) -> AxumResponse {
    let parent = extract_trace_context(request.headers());
    let _ = tracing::Span::current().set_parent(parent);
    next.run(request).await
}

fn extract_trace_context(headers: &axum::http::HeaderMap) -> opentelemetry::Context {
    global::get_text_map_propagator(|propagator| propagator.extract(&HeaderExtractor(headers)))
}

struct HeaderExtractor<'a>(&'a axum::http::HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(axum::http::HeaderName::as_str).collect()
    }
}

fn request_trace_id(request: &Request) -> Option<&str> {
    request
        .extensions()
        .get::<RequestId>()
        .and_then(|request_id| request_id.header_value().to_str().ok())
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn http_panic_response(_panic: Box<dyn Any + Send + 'static>) -> Response<Body> {
    error::internal_error_response()
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic payload")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiError;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;
    use tower_http::cors::CorsLayer;
    use tracing_subscriber::fmt::MakeWriter;

    #[test]
    fn validates_bounded_safe_request_ids() {
        assert!(valid_request_id("01K23ABC-test_1"));
        assert!(!valid_request_id(""));
        assert!(!valid_request_id("contains spaces"));
        assert!(!valid_request_id(&"a".repeat(65)));
    }

    #[test]
    fn extracts_w3c_trace_parent() {
        use opentelemetry::trace::TraceContextExt;

        global::set_text_map_propagator(TraceContextPropagator::new());
        let headers = axum::http::HeaderMap::from_iter([(
            axum::http::header::HeaderName::from_static("traceparent"),
            axum::http::HeaderValue::from_static(
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            ),
        )]);
        let context = extract_trace_context(&headers);
        assert_eq!(
            context.span().span_context().trace_id().to_string(),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
    }

    #[test]
    fn json_formatter_emits_one_json_object_per_line() {
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("info"))
            .json()
            .flatten_event(false)
            .with_current_span(true)
            .with_span_list(false)
            .with_writer(writer.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                event = "test.event",
                trace_id = "json-trace-1",
                "JSON log test"
            );
        });

        let output = writer.contents();
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1);
        let event: Value = serde_json::from_str(lines[0]).expect("valid JSON log line");
        assert_eq!(event["level"], "INFO");
        assert_eq!(event["fields"]["trace_id"], "json-trace-1");
    }

    #[tokio::test]
    async fn error_response_propagates_the_request_id() {
        let app = default_http_observability(Router::new().route(
            "/error",
            get(|| async { Err::<(), _>(ApiError::internal("database password leaked")) }),
        ));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/error")
                    .header(&REQUEST_ID_HEADER, "test-trace-123")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()[&REQUEST_ID_HEADER], "test-trace-123");
        let body = response_json(response).await;
        assert_eq!(body["error"]["traceId"], "test-trace-123");
        assert_eq!(body["error"]["message"], "服务器内部错误");
        assert!(!body.to_string().contains("database password leaked"));
    }

    #[tokio::test]
    async fn invalid_request_id_is_replaced() {
        let app =
            default_http_observability(Router::new().route("/health", get(|| async { "ok" })));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .header(&REQUEST_ID_HEADER, "unsafe request id")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        let request_id = response.headers()[&REQUEST_ID_HEADER]
            .to_str()
            .expect("ASCII request id");
        assert!(valid_request_id(request_id));
        assert_ne!(request_id, "unsafe request id");
    }

    #[tokio::test]
    async fn cors_preflight_response_has_a_request_id() {
        let router = Router::new()
            .route("/resource", get(|| async { "ok" }))
            .layer(CorsLayer::permissive());
        let app = default_http_observability(router);
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("OPTIONS")
                    .uri("/resource")
                    .header("origin", "http://localhost:4200")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert!(response.headers().contains_key(&REQUEST_ID_HEADER));
    }

    #[tokio::test]
    async fn panic_returns_the_standard_error_contract() {
        let app = default_http_observability(Router::new().route("/panic", get(panic_handler)));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/panic")
                    .header(&REQUEST_ID_HEADER, "panic-trace-123")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()[&REQUEST_ID_HEADER], "panic-trace-123");
        let body = response_json(response).await;
        assert_eq!(body["error"]["code"], "INTERNAL_ERROR");
        assert_eq!(body["error"]["traceId"], "panic-trace-123");
    }

    async fn response_json(response: AxumResponse) -> Value {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect response")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("JSON response")
    }

    async fn panic_handler() -> &'static str {
        panic!("simulated handler panic")
    }

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl SharedWriter {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().expect("log buffer lock").clone()).expect("UTF-8 logs")
        }
    }

    impl Write for SharedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("log buffer lock")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for SharedWriter {
        type Writer = Self;

        fn make_writer(&'writer self) -> Self::Writer {
            self.clone()
        }
    }
}
