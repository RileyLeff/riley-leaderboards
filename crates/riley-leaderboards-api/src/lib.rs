use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::{Json, Router, routing::get};
use riley_leaderboards_core::config::RileyLeaderboardsConfig;
use serde_json::json;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub mod auth;
mod error;
mod routes;

pub struct AppState {
    pub pool: PgPool,
    pub config: RileyLeaderboardsConfig,
    pub auth_mode: auth::AuthMode,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let server_config = state
        .config
        .server
        .as_ref()
        .cloned()
        .unwrap_or_default();

    let mut app = Router::new()
        .route("/health", get(health))
        .route(
            "/webhooks/github",
            axum::routing::post(routes::webhooks::github)
                .layer(axum::extract::DefaultBodyLimit::max(5 * 1024 * 1024)),
        )
        .nest("/boards", board_routes(state.clone()))
        .with_state(state);

    // CORS
    if !server_config.cors_origins.is_empty() {
        let cors = build_cors_layer(&server_config.cors_origins);
        app = app.layer(cors);
    }

    // Request tracing
    app = app.layer(
        TraceLayer::new_for_http()
            .make_span_with(tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
            .on_response(
                tower_http::trace::DefaultOnResponse::new().level(tracing::Level::INFO),
            ),
    );

    // Rate limiting
    if server_config.rate_limit_per_second > 0 {
        let governor_conf = Arc::new(
            tower_governor::governor::GovernorConfigBuilder::default()
                .per_second(server_config.rate_limit_per_second)
                .burst_size(server_config.rate_limit_burst)
                .finish()
                .expect("invalid rate limit config"),
        );
        app = app.layer(tower_governor::GovernorLayer {
            config: governor_conf,
        });
    }

    app
}

fn build_cors_layer(origins: &[String]) -> CorsLayer {
    use axum::http::header;
    use tower_http::cors::AllowOrigin;

    let allow_origin = if origins.iter().any(|o| o == "*") {
        AllowOrigin::any()
    } else {
        let parsed: Vec<axum::http::HeaderValue> = origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        AllowOrigin::list(parsed)
    };

    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
}

fn board_routes(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(routes::boards::list).post(routes::boards::create))
        .route(
            "/{slug}",
            get(routes::boards::get)
                .patch(routes::boards::update)
                .delete(routes::boards::delete),
        )
        .route(
            "/{slug}/entries",
            get(routes::entries::list).post(routes::entries::create),
        )
        .route(
            "/{slug}/entries/{entry_slug}",
            get(routes::entries::get)
                .patch(routes::entries::update)
                .delete(routes::entries::delete),
        )
        .route(
            "/{slug}/entries/{entry_slug}/history",
            get(routes::entries::history),
        )
        .route(
            "/{slug}/versions",
            get(routes::versions::list).post(routes::versions::create),
        )
        .route("/{slug}/versions/{version_number}", get(routes::versions::get))
        .route("/{slug}/latest", get(routes::versions::latest))
        .route("/{slug}/diff", get(routes::versions::diff))
        .route("/{slug}/since/{version_number}", get(routes::versions::since))
        .route(
            "/{slug}/references",
            get(routes::references::list).post(routes::references::create),
        )
        .route(
            "/{slug}/references/{reference_id}",
            axum::routing::delete(routes::references::delete),
        )
        .route("/{slug}/scores", axum::routing::post(routes::scores::submit))
        .route(
            "/{slug}/snapshot",
            axum::routing::post(routes::scores::snapshot),
        )
        .layer(middleware::from_fn_with_state(
            state,
            auth::require_auth,
        ))
}

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1::int4")
        .fetch_one(&state.pool)
        .await
    {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "unhealthy", "reason": "database unreachable" })),
        ),
    }
}

pub async fn serve(state: Arc<AppState>) -> anyhow::Result<()> {
    let server_config = state
        .config
        .server
        .as_ref()
        .cloned()
        .unwrap_or_default();

    let app = build_router(state);

    let addr: SocketAddr = format!("{}:{}", server_config.host, server_config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm.recv() => {},
    }
    tracing::info!("shutdown signal received");
}
