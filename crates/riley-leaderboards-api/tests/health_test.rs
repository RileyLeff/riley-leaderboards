//! Integration tests for health, metrics, and OpenAPI endpoints.
//!
//! Requires a running Postgres 18 instance (docker compose up -d).

use std::sync::Arc;

use http::Request;
use http_body_util::BodyExt;
use riley_leaderboards_api::{AppState, build_router};
use riley_leaderboards_core::config::{
    ConfigValue, DatabaseConfig, RileyLeaderboardsConfig, ServerConfig,
};
use riley_leaderboards_core::db;
use tower::ServiceExt;

fn test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://riley_leaderboards:riley_leaderboards_test@localhost:15433/riley_leaderboards_test".to_string()
    })
}

#[tokio::test]
async fn health_returns_ok() {
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some("test_health_endpoint".to_string()),
        },
        redis: None,
        auth: None,
        sync: None,
        limits: None,
        webhooks: vec![],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool: pool.clone(),
        redis: None,
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
        event_bus: None,
        task_tracker: tokio_util::task::TaskTracker::new(),
    });
    let app = build_router(state);

    let response = app
        .oneshot(Request::get("/health").body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");

    // Clean up
    sqlx::query("DROP SCHEMA IF EXISTS \"test_health_endpoint\" CASCADE")
        .execute(&pool)
        .await
        .expect("test cleanup: failed to drop schema");
    pool.close().await;
}

#[tokio::test]
async fn metrics_endpoint_returns_prometheus_format() {
    let config = RileyLeaderboardsConfig {
        server: Some(ServerConfig {
            metrics_enabled: true,
            ..ServerConfig::default()
        }),
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some("test_metrics_endpoint".to_string()),
        },
        redis: None,
        auth: None,
        sync: None,
        limits: None,
        webhooks: vec![],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool: pool.clone(),
        redis: None,
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
        event_bus: None,
        task_tracker: tokio_util::task::TaskTracker::new(),
    });
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::get("/metrics")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    // Prometheus text format: should contain at least a comment or be empty (no requests recorded yet)
    // The endpoint itself is valid if it returns 200 with text content
    assert!(
        text.is_empty() || text.contains('#') || text.contains("http_requests_total"),
        "expected Prometheus text format, got: {text}"
    );

    // Clean up
    sqlx::query("DROP SCHEMA IF EXISTS \"test_metrics_endpoint\" CASCADE")
        .execute(&pool)
        .await
        .expect("test cleanup: failed to drop schema");
    pool.close().await;
}

#[tokio::test]
async fn openapi_spec_returns_valid_json() {
    let config = RileyLeaderboardsConfig {
        server: Some(ServerConfig {
            docs_enabled: true,
            ..ServerConfig::default()
        }),
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some("test_openapi_endpoint".to_string()),
        },
        redis: None,
        auth: None,
        sync: None,
        limits: None,
        webhooks: vec![],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool: pool.clone(),
        redis: None,
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
        event_bus: None,
        task_tracker: tokio_util::task::TaskTracker::new(),
    });
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::get("/api-doc/openapi.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify basic OpenAPI structure
    assert_eq!(json["info"]["title"], "Riley Leaderboards API");
    // 28 handlers across 18 unique paths (multiple methods per path are grouped)
    assert!(json["paths"].as_object().map_or(false, |p| p.len() >= 15),
        "expected at least 15 paths in OpenAPI spec, got {}",
        json["paths"].as_object().map_or(0, |p| p.len()));

    // Clean up
    sqlx::query("DROP SCHEMA IF EXISTS \"test_openapi_endpoint\" CASCADE")
        .execute(&pool)
        .await
        .expect("test cleanup: failed to drop schema");
    pool.close().await;
}
