//! Prometheus metrics endpoint and request instrumentation.

use std::sync::OnceLock;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::PrometheusHandle;

/// Global handle — ensures install_recorder() is called at most once,
/// even if build_router() is called multiple times (e.g. in tests).
static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the global Prometheus metrics recorder and return a handle
/// for rendering the `/metrics` endpoint. Idempotent — subsequent calls
/// return the existing handle.
pub fn init() -> PrometheusHandle {
    PROMETHEUS_HANDLE
        .get_or_init(|| {
            metrics_exporter_prometheus::PrometheusBuilder::new()
                .install_recorder()
                .expect("failed to install Prometheus metrics recorder")
        })
        .clone()
}

/// Axum handler for `GET /metrics` — renders all registered metrics
/// in Prometheus text exposition format.
pub async fn handler(
    axum::extract::State(handle): axum::extract::State<PrometheusHandle>,
) -> impl IntoResponse {
    handle.render()
}

/// Axum middleware that records per-request metrics:
/// - `http_requests_total` counter (method, path, status)
/// - `http_request_duration_seconds` histogram (method, path, status)
pub async fn track(request: Request, next: Next) -> Response {
    let method = request.method().clone().to_string();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|mp| mp.as_str().to_string())
        .unwrap_or_else(|| "unmatched".to_string());

    let start = Instant::now();
    let response = next.run(request).await;
    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    let labels = [
        ("method", method),
        ("path", path),
        ("status", status),
    ];

    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_request_duration_seconds", &labels).record(duration);

    response
}
