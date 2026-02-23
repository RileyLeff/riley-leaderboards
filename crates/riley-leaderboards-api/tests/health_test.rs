//! Integration test for the health endpoint.
//!
//! Requires a running Postgres 18 instance (docker compose up -d).

use std::sync::Arc;

use http::Request;
use http_body_util::BodyExt;
use riley_leaderboards_api::{AppState, build_router};
use riley_leaderboards_core::config::{ConfigValue, DatabaseConfig, RileyLeaderboardsConfig};
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
