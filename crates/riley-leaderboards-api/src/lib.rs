use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router, routing::get};
use riley_leaderboards_core::config::RileyLeaderboardsConfig;
use serde_json::json;
use sqlx::PgPool;

mod error;
mod routes;

pub struct AppState {
    pub pool: PgPool,
    pub config: RileyLeaderboardsConfig,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/boards", board_routes())
        .with_state(state)
}

fn board_routes() -> Router<Arc<AppState>> {
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
