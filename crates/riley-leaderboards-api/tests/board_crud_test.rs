//! Integration tests for Board, Entry, and Version CRUD via HTTP API.
//!
//! Requires a running Postgres 18 instance (docker compose up -d).

use std::sync::Arc;

use axum::body::Body;
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

async fn setup(schema: &str) -> (Arc<AppState>, axum::Router) {
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool,
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let router = build_router(state.clone());
    (state, router)
}

async fn cleanup(state: &AppState, schema: &str) {
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema))
        .execute(&state.pool)
        .await
        .expect("test cleanup: failed to drop schema");
    state.pool.close().await;
}

async fn json_body(response: http::Response<Body>) -> serde_json::Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn json_request(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");

    if let Some(b) = body {
        builder.body(Body::from(serde_json::to_vec(&b).unwrap())).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

// ── Board CRUD ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn board_create_list_get_update_delete() {
    let schema = "test_board_crud";
    let (state, app) = setup(schema).await;

    // Create
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "test-board",
            "name": "Test Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let board = json_body(resp).await;
    assert_eq!(board["slug"], "test-board");
    assert_eq!(board["name"], "Test Board");
    assert_eq!(board["board_type"], "ordered");
    assert_eq!(board["sort_direction"], "desc");
    assert_eq!(board["accumulative"], false);

    // List
    let resp = app.clone().oneshot(json_request("GET", "/boards", None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let boards = json_body(resp).await;
    assert_eq!(boards["items"].as_array().unwrap().len(), 1);

    // Get (returns BoardSummary with latest_version and entry_count)
    let resp = app.clone().oneshot(json_request("GET", "/boards/test-board", None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let summary = json_body(resp).await;
    assert_eq!(summary["slug"], "test-board");
    assert_eq!(summary["latest_version"], serde_json::Value::Null);
    assert_eq!(summary["entry_count"], 0);

    // Update
    let resp = app.clone().oneshot(json_request(
        "PATCH",
        "/boards/test-board",
        Some(serde_json::json!({
            "name": "Updated Board"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let updated = json_body(resp).await;
    assert_eq!(updated["name"], "Updated Board");

    // Delete
    let resp = app.clone().oneshot(json_request("DELETE", "/boards/test-board", None)).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Verify deleted
    let resp = app.clone().oneshot(json_request("GET", "/boards/test-board", None)).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn board_create_duplicate_slug_returns_409() {
    let schema = "test_board_dup";
    let (state, app) = setup(schema).await;

    let body = serde_json::json!({
        "slug": "dup-board",
        "name": "First",
        "board_type": "ordered"
    });

    let resp = app.clone().oneshot(json_request("POST", "/boards", Some(body.clone()))).await.unwrap();
    assert_eq!(resp.status(), 201);

    let resp = app.clone().oneshot(json_request("POST", "/boards", Some(body))).await.unwrap();
    assert_eq!(resp.status(), 409);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn board_create_invalid_type_returns_400() {
    let schema = "test_board_invalid";
    let (state, app) = setup(schema).await;

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "bad",
            "name": "Bad",
            "board_type": "invalid"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Entry CRUD ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn entry_create_list_get_update_delete() {
    let schema = "test_entry_crud";
    let (state, app) = setup(schema).await;

    // Create board first
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "sandwich-board",
            "name": "Sandwiches",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Create entry
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/sandwich-board/entries",
        Some(serde_json::json!({
            "slug": "crunchy-boi",
            "name": "Compliments Only Crunchy Boi",
            "metadata": { "address": "1026 Vermont Ave NW" }
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let entry = json_body(resp).await;
    assert_eq!(entry["slug"], "crunchy-boi");
    assert!(entry["metadata"]["address"].as_str().is_some());

    // List entries
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/sandwich-board/entries",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let entries = json_body(resp).await;
    assert_eq!(entries["items"].as_array().unwrap().len(), 1);

    // Get entry
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/sandwich-board/entries/crunchy-boi",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let entry = json_body(resp).await;
    assert_eq!(entry["name"], "Compliments Only Crunchy Boi");

    // Update entry
    let resp = app.clone().oneshot(json_request(
        "PATCH",
        "/boards/sandwich-board/entries/crunchy-boi",
        Some(serde_json::json!({
            "name": "The Crunchy Boi"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let updated = json_body(resp).await;
    assert_eq!(updated["name"], "The Crunchy Boi");

    // Delete entry
    let resp = app.clone().oneshot(json_request(
        "DELETE",
        "/boards/sandwich-board/entries/crunchy-boi",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 204);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn entry_on_nonexistent_board_returns_404() {
    let schema = "test_entry_no_board";
    let (state, app) = setup(schema).await;

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/nonexistent/entries",
        Some(serde_json::json!({
            "slug": "test",
            "name": "Test"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

// ── Version CRUD (ordered) ────────────────────────────────────────────────

#[tokio::test]
async fn ordered_version_create_and_fetch() {
    let schema = "test_ordered_version";
    let (state, app) = setup(schema).await;

    // Create board
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "dc-sandwiches",
            "name": "Best Sandwiches in DC",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    // Create entries
    for (slug, name) in [("crunchy-boi", "Crunchy Boi"), ("humberto", "Humberto"), ("litteri", "A. Litteri")] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/dc-sandwiches/entries",
            Some(serde_json::json!({ "slug": slug, "name": name })),
        )).await.unwrap();
    }

    // Create version with placements
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/dc-sandwiches/versions",
        Some(serde_json::json!({
            "note": "Initial rankings",
            "placements": [
                { "entry_slug": "crunchy-boi", "position": 1 },
                { "entry_slug": "humberto", "position": 2 },
                { "entry_slug": "litteri", "position": 3 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    assert_eq!(version["version_number"], 1);
    assert_eq!(version["note"], "Initial rankings");
    assert_eq!(version["placements"].as_array().unwrap().len(), 3);
    assert_eq!(version["placements"][0]["entry_slug"], "crunchy-boi");
    assert_eq!(version["placements"][0]["position"], 1);

    // Get version by number
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/dc-sandwiches/versions/1",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let fetched = json_body(resp).await;
    assert_eq!(fetched["version_number"], 1);
    assert_eq!(fetched["placements"].as_array().unwrap().len(), 3);

    // Get latest
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/dc-sandwiches/latest",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let latest = json_body(resp).await;
    assert_eq!(latest["version_number"], 1);

    // Create second version
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/dc-sandwiches/versions",
        Some(serde_json::json!({
            "note": "Updated rankings",
            "placements": [
                { "entry_slug": "humberto", "position": 1 },
                { "entry_slug": "crunchy-boi", "position": 2 },
                { "entry_slug": "litteri", "position": 3 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let v2 = json_body(resp).await;
    assert_eq!(v2["version_number"], 2);

    // List versions
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/dc-sandwiches/versions",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let versions = json_body(resp).await;
    assert_eq!(versions["items"].as_array().unwrap().len(), 2);
    // Newest first
    assert_eq!(versions["items"][0]["version_number"], 2);
    assert_eq!(versions["items"][1]["version_number"], 1);

    // Latest should now be version 2
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/dc-sandwiches/latest",
        None,
    )).await.unwrap();
    let latest = json_body(resp).await;
    assert_eq!(latest["version_number"], 2);
    assert_eq!(latest["placements"][0]["entry_slug"], "humberto");

    // Board summary should reflect entry count and latest version
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/dc-sandwiches",
        None,
    )).await.unwrap();
    let summary = json_body(resp).await;
    assert_eq!(summary["latest_version"], 2);
    assert_eq!(summary["entry_count"], 3);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn ordered_version_implicit_positions() {
    let schema = "test_ordered_implicit";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "books",
            "name": "Best Books",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    for slug in ["book-a", "book-b", "book-c"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/books/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    // Create version WITHOUT explicit positions — should use array order
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/books/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "book-c" },
                { "entry_slug": "book-a" },
                { "entry_slug": "book-b" }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    let placements = version["placements"].as_array().unwrap();
    // Positions derived from array order: c=1, a=2, b=3
    // Returned sorted by position ASC
    assert_eq!(placements[0]["entry_slug"], "book-c");
    assert_eq!(placements[0]["position"], 1);
    assert_eq!(placements[1]["entry_slug"], "book-a");
    assert_eq!(placements[1]["position"], 2);
    assert_eq!(placements[2]["entry_slug"], "book-b");
    assert_eq!(placements[2]["position"], 3);

    cleanup(&state, schema).await;
}

// ── Scored board ───────────────────────────────────────────────────────────

#[tokio::test]
async fn scored_board_derives_positions() {
    let schema = "test_scored_board";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "high-scores",
            "name": "High Scores",
            "board_type": "scored",
            "sort_direction": "desc"
        })),
    )).await.unwrap();

    for (slug, name) in [("player-a", "Alice"), ("player-b", "Bob"), ("player-c", "Charlie")] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/high-scores/entries",
            Some(serde_json::json!({ "slug": slug, "name": name })),
        )).await.unwrap();
    }

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/high-scores/versions",
        Some(serde_json::json!({
            "note": "Round 1",
            "placements": [
                { "entry_slug": "player-a", "score": 100.0 },
                { "entry_slug": "player-b", "score": 250.0 },
                { "entry_slug": "player-c", "score": 175.0 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    let placements = version["placements"].as_array().unwrap();

    // Sort direction is desc, so Bob (250) should be position 1
    let bob = placements.iter().find(|p| p["entry_slug"] == "player-b").unwrap();
    let charlie = placements.iter().find(|p| p["entry_slug"] == "player-c").unwrap();
    let alice = placements.iter().find(|p| p["entry_slug"] == "player-a").unwrap();

    assert_eq!(bob["position"], 1);
    assert_eq!(charlie["position"], 2);
    assert_eq!(alice["position"], 3);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn scored_board_asc_direction() {
    let schema = "test_scored_asc";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "golf",
            "name": "Golf Scores",
            "board_type": "scored",
            "sort_direction": "asc"
        })),
    )).await.unwrap();

    for slug in ["alice", "bob"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/golf/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/golf/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "alice", "score": 72.0 },
                { "entry_slug": "bob", "score": 68.0 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    let placements = version["placements"].as_array().unwrap();

    // asc: Bob (68) should be position 1
    let bob = placements.iter().find(|p| p["entry_slug"] == "bob").unwrap();
    let alice = placements.iter().find(|p| p["entry_slug"] == "alice").unwrap();
    assert_eq!(bob["position"], 1);
    assert_eq!(alice["position"], 2);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn scored_board_requires_score() {
    let schema = "test_scored_no_score";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "scores",
            "name": "Scores",
            "board_type": "scored"
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/scores/entries",
        Some(serde_json::json!({ "slug": "p1", "name": "Player 1" })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/scores/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "p1" }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Tiered board ───────────────────────────────────────────────────────────

#[tokio::test]
async fn tiered_board_with_tier_config() {
    let schema = "test_tiered_board";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "tier-list",
            "name": "Tier List",
            "board_type": "tiered",
            "tier_config": {
                "tiers": [
                    { "key": "s", "label": "S Tier", "position": 1 },
                    { "key": "a", "label": "A Tier", "position": 2 },
                    { "key": "b", "label": "B Tier", "position": 3 }
                ]
            }
        })),
    )).await.unwrap();

    for slug in ["item-x", "item-y", "item-z"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/tier-list/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/tier-list/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "item-x", "tier": "s", "position": 1 },
                { "entry_slug": "item-y", "tier": "a", "position": 1 },
                { "entry_slug": "item-z", "tier": "b", "position": 1 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    let placements = version["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 3);

    let x = placements.iter().find(|p| p["entry_slug"] == "item-x").unwrap();
    assert_eq!(x["tier"], "s");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn tiered_board_requires_tier() {
    let schema = "test_tiered_no_tier";
    let (state, app) = setup(schema).await;

    // Creating a tiered board without tier_config should be rejected
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "tiers",
            "name": "Tiers",
            "board_type": "tiered"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    // With tier_config, board creation succeeds but placements missing tier are rejected
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "tiers",
            "name": "Tiers",
            "board_type": "tiered",
            "tier_config": {
                "tiers": [{ "key": "s", "label": "S Tier", "position": 1 }]
            }
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/tiers/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/tiers/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "item", "position": 1 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn tiered_board_invalid_tier_returns_400() {
    let schema = "test_tiered_invalid";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "tiers",
            "name": "Tiers",
            "board_type": "tiered",
            "tier_config": {
                "tiers": [
                    { "key": "s", "label": "S", "position": 1 }
                ]
            }
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/tiers/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/tiers/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "item", "tier": "nonexistent" }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Validation ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn version_with_nonexistent_entry_returns_400() {
    let schema = "test_version_no_entry";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "nonexistent" }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn version_with_duplicate_entries_returns_400() {
    let schema = "test_version_dup_entry";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "item", "position": 1 },
                { "entry_slug": "item", "position": 2 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn version_with_empty_placements_returns_400() {
    let schema = "test_version_empty";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": []
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Cascade delete ─────────────────────────────────────────────────────────

#[tokio::test]
async fn board_delete_cascades_entries_and_versions() {
    let schema = "test_cascade_delete";
    let (state, app) = setup(schema).await;

    // Create board with entries and a version
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "ephemeral",
            "name": "Ephemeral",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/ephemeral/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/ephemeral/versions",
        Some(serde_json::json!({
            "placements": [{ "entry_slug": "item", "position": 1 }]
        })),
    )).await.unwrap();

    // Delete the board
    let resp = app.clone().oneshot(json_request("DELETE", "/boards/ephemeral", None)).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Board, entries, versions all gone
    let resp = app.clone().oneshot(json_request("GET", "/boards/ephemeral", None)).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

// ── Slug validation ────────────────────────────────────────────────────────

#[tokio::test]
async fn invalid_board_slug_returns_400() {
    let schema = "test_invalid_slug";
    let (state, app) = setup(schema).await;

    // Uppercase
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "Bad-Slug",
            "name": "Bad",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    // Spaces
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "bad slug",
            "name": "Bad",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    // Leading hyphen
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "-bad",
            "name": "Bad",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn invalid_entry_slug_returns_400() {
    let schema = "test_entry_invalid_slug";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/entries",
        Some(serde_json::json!({
            "slug": "Bad Entry!",
            "name": "Bad"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Nullable PATCH semantics ───────────────────────────────────────────────

#[tokio::test]
async fn board_patch_can_clear_metadata_to_null() {
    let schema = "test_patch_clear";
    let (state, app) = setup(schema).await;

    // Create board with metadata
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "meta-board",
            "name": "Board with Metadata",
            "board_type": "ordered",
            "metadata": { "description": "some description" }
        })),
    )).await.unwrap();

    // Verify metadata is set
    let resp = app.clone().oneshot(json_request("GET", "/boards/meta-board", None)).await.unwrap();
    let board = json_body(resp).await;
    assert!(board["metadata"].is_object());

    // PATCH with metadata: null to clear it
    let resp = app.clone().oneshot(json_request(
        "PATCH",
        "/boards/meta-board",
        Some(serde_json::json!({
            "metadata": null
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let updated = json_body(resp).await;
    assert!(updated["metadata"].is_null());

    // Verify it persisted
    let resp = app.clone().oneshot(json_request("GET", "/boards/meta-board", None)).await.unwrap();
    let board = json_body(resp).await;
    assert!(board["metadata"].is_null());

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn board_patch_omitted_fields_keep_old_values() {
    let schema = "test_patch_omit";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "keep-board",
            "name": "Original",
            "board_type": "ordered",
            "metadata": { "key": "value" }
        })),
    )).await.unwrap();

    // PATCH only name — metadata should stay
    let resp = app.clone().oneshot(json_request(
        "PATCH",
        "/boards/keep-board",
        Some(serde_json::json!({
            "name": "Updated Name"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let updated = json_body(resp).await;
    assert_eq!(updated["name"], "Updated Name");
    assert_eq!(updated["metadata"]["key"], "value");

    cleanup(&state, schema).await;
}

// ── Entry deletion with placements ─────────────────────────────────────────

#[tokio::test]
async fn entry_delete_with_placements_returns_409() {
    let schema = "test_entry_del_conflict";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();

    // Create a version with this entry
    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": [{ "entry_slug": "item", "position": 1 }]
        })),
    )).await.unwrap();

    // Try to delete the entry — should be rejected
    let resp = app.clone().oneshot(json_request(
        "DELETE",
        "/boards/board/entries/item",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 409);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn entry_delete_without_placements_succeeds() {
    let schema = "test_entry_del_ok";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/entries",
        Some(serde_json::json!({ "slug": "unused", "name": "Unused" })),
    )).await.unwrap();

    // Delete entry with no placements — should work
    let resp = app.clone().oneshot(json_request(
        "DELETE",
        "/boards/board/entries/unused",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 204);

    cleanup(&state, schema).await;
}

// ── Ordered board duplicate positions ──────────────────────────────────────

#[tokio::test]
async fn ordered_board_duplicate_positions_returns_400() {
    let schema = "test_dup_positions";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    for slug in ["a", "b"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/board/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "a", "position": 1 },
                { "entry_slug": "b", "position": 1 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Review Round 2 fix tests ─────────────────────────────────────────────

#[tokio::test]
async fn ordered_board_mixed_explicit_implicit_position_collision() {
    let schema = "test_mixed_pos_collision";
    let (state, app) = setup(schema).await;

    // Create board and entries
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    for slug in ["a", "b"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/board/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    // Entry "a" implicit position = 1, entry "b" explicit position = 1 → collision
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "a" },
                { "entry_slug": "b", "position": 1 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("duplicate position"));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn tiered_board_invalid_tier_config_shape_returns_400() {
    let schema = "test_invalid_tier_config";
    let (state, app) = setup(schema).await;

    // Missing 'position' field in tier
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "tiered",
            "tier_config": { "tiers": [{ "key": "s" }] }
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("integer 'position'"));

    // tiers is not an array
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board2",
            "name": "Board2",
            "board_type": "tiered",
            "tier_config": { "tiers": "not-an-array" }
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn empty_name_returns_400() {
    let schema = "test_empty_name";
    let (state, app) = setup(schema).await;

    // Empty board name
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("name must not be empty"));

    // Create valid board, then test empty entry name
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/entries",
        Some(serde_json::json!({ "slug": "entry", "name": "" })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("name must not be empty"));

    cleanup(&state, schema).await;
}

// ── Phase 3: History + Diffing ────────────────────────────────────────────

#[tokio::test]
async fn entry_history_across_versions() {
    let schema = "test_entry_history";
    let (state, app) = setup(schema).await;

    // Create board and entries
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "sandwiches",
            "name": "Sandwiches",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    for slug in ["crunchy-boi", "humberto", "litteri"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/sandwiches/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    // Version 1: crunchy=1, humberto=2, litteri=3
    app.clone().oneshot(json_request(
        "POST",
        "/boards/sandwiches/versions",
        Some(serde_json::json!({
            "note": "v1",
            "placements": [
                { "entry_slug": "crunchy-boi", "position": 1 },
                { "entry_slug": "humberto", "position": 2 },
                { "entry_slug": "litteri", "position": 3 }
            ]
        })),
    )).await.unwrap();

    // Version 2: humberto=1, crunchy=2, litteri=3
    app.clone().oneshot(json_request(
        "POST",
        "/boards/sandwiches/versions",
        Some(serde_json::json!({
            "note": "v2",
            "placements": [
                { "entry_slug": "humberto", "position": 1 },
                { "entry_slug": "crunchy-boi", "position": 2 },
                { "entry_slug": "litteri", "position": 3 }
            ]
        })),
    )).await.unwrap();

    // Get history for crunchy-boi (moved from pos 1 → 2)
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/sandwiches/entries/crunchy-boi/history",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let history = json_body(resp).await;
    let items = history.as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["version_number"], 1);
    assert_eq!(items[0]["position"], 1);
    assert_eq!(items[1]["version_number"], 2);
    assert_eq!(items[1]["position"], 2);

    // History for entry not in any version yet returns empty array
    app.clone().oneshot(json_request(
        "POST",
        "/boards/sandwiches/entries",
        Some(serde_json::json!({ "slug": "new-entry", "name": "New" })),
    )).await.unwrap();
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/sandwiches/entries/new-entry/history",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let empty = json_body(resp).await;
    assert_eq!(empty.as_array().unwrap().len(), 0);

    // History for nonexistent entry returns 404
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/sandwiches/entries/nonexistent/history",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn entry_history_scored_board() {
    let schema = "test_history_scored";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "scores",
            "name": "Scores",
            "board_type": "scored",
            "sort_direction": "desc"
        })),
    )).await.unwrap();

    for slug in ["alice", "bob"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/scores/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    // V1: alice=100, bob=200
    app.clone().oneshot(json_request(
        "POST",
        "/boards/scores/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "alice", "score": 100.0 },
                { "entry_slug": "bob", "score": 200.0 }
            ]
        })),
    )).await.unwrap();

    // V2: alice=300, bob=200
    app.clone().oneshot(json_request(
        "POST",
        "/boards/scores/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "alice", "score": 300.0 },
                { "entry_slug": "bob", "score": 200.0 }
            ]
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/scores/entries/alice/history",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let history = json_body(resp).await;
    let items = history.as_array().unwrap();
    assert_eq!(items.len(), 2);
    // V1: alice scored 100, position 2 (bob was higher)
    assert_eq!(items[0]["version_number"], 1);
    assert_eq!(items[0]["score"], 100.0);
    assert_eq!(items[0]["position"], 2);
    // V2: alice scored 300, position 1
    assert_eq!(items[1]["version_number"], 2);
    assert_eq!(items[1]["score"], 300.0);
    assert_eq!(items[1]["position"], 1);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn version_diff_ordered_board() {
    let schema = "test_diff_ordered";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    for slug in ["a", "b", "c", "d"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/board/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    // V1: a=1, b=2, c=3
    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "a", "position": 1 },
                { "entry_slug": "b", "position": 2 },
                { "entry_slug": "c", "position": 3 }
            ]
        })),
    )).await.unwrap();

    // V2: b=1, a=2, d=3 (c removed, d added, a and b swapped)
    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "b", "position": 1 },
                { "entry_slug": "a", "position": 2 },
                { "entry_slug": "d", "position": 3 }
            ]
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/board/diff?from=1&to=2",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let diff = json_body(resp).await;

    assert_eq!(diff["from_version"], 1);
    assert_eq!(diff["to_version"], 2);

    // Added: d
    let added = diff["added"].as_array().unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["entry_slug"], "d");
    assert_eq!(added[0]["position"], 3);

    // Removed: c
    let removed = diff["removed"].as_array().unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0]["entry_slug"], "c");

    // Moved: a (1→2) and b (2→1)
    let moved = diff["moved"].as_array().unwrap();
    assert_eq!(moved.len(), 2);
    let moved_a = moved.iter().find(|m| m["entry_slug"] == "a").unwrap();
    assert_eq!(moved_a["from_position"], 1);
    assert_eq!(moved_a["to_position"], 2);
    let moved_b = moved.iter().find(|m| m["entry_slug"] == "b").unwrap();
    assert_eq!(moved_b["from_position"], 2);
    assert_eq!(moved_b["to_position"], 1);

    // Unchanged: none
    assert_eq!(diff["unchanged"].as_array().unwrap().len(), 0);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn version_diff_tiered_board() {
    let schema = "test_diff_tiered";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "tiers",
            "name": "Tiers",
            "board_type": "tiered",
            "tier_config": {
                "tiers": [
                    { "key": "s", "label": "S", "position": 1 },
                    { "key": "a", "label": "A", "position": 2 }
                ]
            }
        })),
    )).await.unwrap();

    for slug in ["x", "y"] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/tiers/entries",
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )).await.unwrap();
    }

    // V1: x in tier A, y in tier A
    app.clone().oneshot(json_request(
        "POST",
        "/boards/tiers/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "x", "tier": "a", "position": 1 },
                { "entry_slug": "y", "tier": "a", "position": 2 }
            ]
        })),
    )).await.unwrap();

    // V2: x promoted to S, y stays in A
    app.clone().oneshot(json_request(
        "POST",
        "/boards/tiers/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "x", "tier": "s", "position": 1 },
                { "entry_slug": "y", "tier": "a", "position": 1 }
            ]
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/tiers/diff?from=1&to=2",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let diff = json_body(resp).await;

    // x moved (tier change from a→s)
    let moved = diff["moved"].as_array().unwrap();
    let moved_x = moved.iter().find(|m| m["entry_slug"] == "x").unwrap();
    assert_eq!(moved_x["from_tier"], "a");
    assert_eq!(moved_x["to_tier"], "s");

    // y: position changed from 2→1 within tier a
    let moved_y = moved.iter().find(|m| m["entry_slug"] == "y").unwrap();
    assert_eq!(moved_y["from_position"], 2);
    assert_eq!(moved_y["to_position"], 1);

    assert_eq!(diff["added"].as_array().unwrap().len(), 0);
    assert_eq!(diff["removed"].as_array().unwrap().len(), 0);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn version_diff_missing_params_returns_400() {
    let schema = "test_diff_missing_params";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    // Missing both params
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/board/diff",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    // Missing 'to'
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/board/diff?from=1",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn version_diff_nonexistent_version_returns_404() {
    let schema = "test_diff_no_version";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/board/diff?from=1&to=2",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn since_returns_newer_versions() {
    let schema = "test_since";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();

    // Create 3 versions
    for i in 1..=3 {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/board/versions",
            Some(serde_json::json!({
                "note": format!("v{i}"),
                "placements": [{ "entry_slug": "item", "position": 1 }]
            })),
        )).await.unwrap();
    }

    // since/1 should return v2 and v3
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/board/since/1",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let versions = json_body(resp).await;
    let arr = versions.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["version_number"], 2);
    assert_eq!(arr[1]["version_number"], 3);

    // since/3 should return empty
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/board/since/3",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let versions = json_body(resp).await;
    assert_eq!(versions.as_array().unwrap().len(), 0);

    // since/0 should return all 3
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/board/since/0",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let versions = json_body(resp).await;
    assert_eq!(versions.as_array().unwrap().len(), 3);

    cleanup(&state, schema).await;
}

// ── Phase 4: References ──────────────────────────────────────────────────

#[tokio::test]
async fn reference_create_list_delete() {
    let schema = "test_ref_crud";
    let (state, app) = setup(schema).await;

    // Create board with an entry and version
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "sandwiches",
            "name": "Sandwiches",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/sandwiches/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/sandwiches/versions",
        Some(serde_json::json!({
            "note": "v1",
            "placements": [{ "entry_slug": "item", "position": 1 }]
        })),
    )).await.unwrap();

    // Create a reference pinned to version 1
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/sandwiches/references",
        Some(serde_json::json!({
            "pinned_version_number": 1,
            "uri": "/blog/sandwich-rankings",
            "ref_type": "embed",
            "label": "Blog Post"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let reference = json_body(resp).await;
    assert_eq!(reference["uri"], "/blog/sandwich-rankings");
    assert_eq!(reference["ref_type"], "embed");
    assert_eq!(reference["label"], "Blog Post");
    assert!(reference["pinned_version_id"].is_string()); // resolved to UUID
    assert_eq!(reference["pinned_version_number"], 1); // version number included
    let ref_id = reference["id"].as_str().unwrap().to_string();

    // Create a reference without pinned version (follow latest)
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/sandwiches/references",
        Some(serde_json::json!({
            "uri": "https://forestroyale.rileyleff.com",
            "ref_type": "context"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let ref2 = json_body(resp).await;
    assert!(ref2["pinned_version_id"].is_null());
    assert!(ref2["pinned_version_number"].is_null()); // null when unpinned

    // List references
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/sandwiches/references",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let refs = json_body(resp).await;
    assert_eq!(refs["items"].as_array().unwrap().len(), 2);

    // Delete first reference
    let resp = app.clone().oneshot(json_request(
        "DELETE",
        &format!("/boards/sandwiches/references/{ref_id}"),
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Verify only one remains
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/sandwiches/references",
        None,
    )).await.unwrap();
    let refs = json_body(resp).await;
    assert_eq!(refs["items"].as_array().unwrap().len(), 1);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn reference_invalid_ref_type_returns_400() {
    let schema = "test_ref_invalid_type";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/references",
        Some(serde_json::json!({
            "uri": "/blog",
            "ref_type": "invalid"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("invalid ref_type"));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn reference_nonexistent_version_returns_404() {
    let schema = "test_ref_no_version";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/references",
        Some(serde_json::json!({
            "pinned_version_number": 999,
            "uri": "/blog",
            "ref_type": "embed"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn reference_delete_nonexistent_returns_404() {
    let schema = "test_ref_del_404";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "DELETE",
        "/boards/board/references/00000000-0000-0000-0000-000000000000",
        None,
    )).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn board_delete_cascades_references() {
    let schema = "test_ref_cascade";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    // Create reference (no pinned version)
    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/references",
        Some(serde_json::json!({
            "uri": "/page",
            "ref_type": "citation"
        })),
    )).await.unwrap();

    // Delete board — references should cascade
    let resp = app.clone().oneshot(json_request("DELETE", "/boards/board", None)).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Board gone
    let resp = app.clone().oneshot(json_request("GET", "/boards/board", None)).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn reference_empty_uri_returns_400() {
    let schema = "test_ref_empty_uri";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/references",
        Some(serde_json::json!({
            "uri": "",
            "ref_type": "embed"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("uri must not be empty"));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn reference_label_too_long_returns_400() {
    let schema = "test_ref_long_label";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    let long_label = "x".repeat(257);
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/references",
        Some(serde_json::json!({
            "uri": "/blog",
            "ref_type": "embed",
            "label": long_label
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("label must not exceed 256"));

    cleanup(&state, schema).await;
}

// ── Phase 5: Accumulative Boards ──────────────────────────────────────

#[tokio::test]
async fn accumulative_score_submit_and_snapshot() {
    let schema = "test_accum_basic";
    let (state, app) = setup(schema).await;

    // Create accumulative scored board
    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "forest-royale",
            "name": "Forest Royale High Scores",
            "board_type": "scored",
            "accumulative": true,
            "sort_direction": "desc"
        })),
    )).await.unwrap();

    // Submit scores (creates entries automatically)
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/forest-royale/scores",
        Some(serde_json::json!({
            "entry_slug": "rileyleff",
            "entry_name": "rileyleff",
            "score": 847.0
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let score = json_body(resp).await;
    assert_eq!(score["score"], 847.0);

    app.clone().oneshot(json_request(
        "POST",
        "/boards/forest-royale/scores",
        Some(serde_json::json!({
            "entry_slug": "alice",
            "entry_name": "Alice",
            "score": 1200.0
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/forest-royale/scores",
        Some(serde_json::json!({
            "entry_slug": "bob",
            "entry_name": "Bob",
            "score": 500.0
        })),
    )).await.unwrap();

    // Snapshot
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/forest-royale/snapshot",
        Some(serde_json::json!({
            "note": "Daily standings"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    assert_eq!(version["version_number"], 1);
    assert_eq!(version["note"], "Daily standings");
    let placements = version["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 3);

    // desc: Alice (1200) #1, rileyleff (847) #2, Bob (500) #3
    assert_eq!(placements[0]["entry_slug"], "alice");
    assert_eq!(placements[0]["position"], 1);
    assert_eq!(placements[0]["score"], 1200.0);
    assert_eq!(placements[1]["entry_slug"], "rileyleff");
    assert_eq!(placements[1]["position"], 2);
    assert_eq!(placements[2]["entry_slug"], "bob");
    assert_eq!(placements[2]["position"], 3);

    // Entries were auto-created — list should show them
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/forest-royale/entries",
        None,
    )).await.unwrap();
    let entries = json_body(resp).await;
    assert_eq!(entries["items"].as_array().unwrap().len(), 3);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn accumulative_score_upsert_behavior() {
    let schema = "test_accum_upsert";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "scored",
            "accumulative": true
        })),
    )).await.unwrap();

    // Submit initial score
    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/scores",
        Some(serde_json::json!({
            "entry_slug": "player",
            "entry_name": "Player",
            "score": 100.0
        })),
    )).await.unwrap();

    // Submit higher score with updated name — should overwrite both
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/scores",
        Some(serde_json::json!({
            "entry_slug": "player",
            "entry_name": "Player Updated",
            "score": 999.0
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let score = json_body(resp).await;
    assert_eq!(score["score"], 999.0);

    // Snapshot should use latest score and updated name
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/snapshot",
        Some(serde_json::json!({})),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    assert_eq!(version["placements"][0]["score"], 999.0);
    assert_eq!(version["placements"][0]["entry_name"], "Player Updated");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn accumulative_asc_sort_direction() {
    let schema = "test_accum_asc";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "golf",
            "name": "Golf",
            "board_type": "scored",
            "accumulative": true,
            "sort_direction": "asc"
        })),
    )).await.unwrap();

    for (slug, name, score) in [("alice", "Alice", 72.0), ("bob", "Bob", 68.0)] {
        app.clone().oneshot(json_request(
            "POST",
            "/boards/golf/scores",
            Some(serde_json::json!({
                "entry_slug": slug,
                "entry_name": name,
                "score": score
            })),
        )).await.unwrap();
    }

    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/golf/snapshot",
        Some(serde_json::json!({})),
    )).await.unwrap();
    let version = json_body(resp).await;
    let placements = version["placements"].as_array().unwrap();

    // asc: Bob (68) #1, Alice (72) #2
    let bob = placements.iter().find(|p| p["entry_slug"] == "bob").unwrap();
    let alice = placements.iter().find(|p| p["entry_slug"] == "alice").unwrap();
    assert_eq!(bob["position"], 1);
    assert_eq!(alice["position"], 2);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn accumulative_score_on_non_accumulative_returns_400() {
    let schema = "test_accum_reject_curated";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();

    // Score submission on non-accumulative board should fail
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/scores",
        Some(serde_json::json!({
            "entry_slug": "x",
            "entry_name": "X",
            "score": 100.0
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("accumulative"));

    // Snapshot on non-accumulative board should fail
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/snapshot",
        Some(serde_json::json!({})),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn accumulative_version_create_rejected() {
    let schema = "test_accum_no_version";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "scored",
            "accumulative": true
        })),
    )).await.unwrap();

    // Submit a score to create an entry
    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/scores",
        Some(serde_json::json!({
            "entry_slug": "player",
            "entry_name": "Player",
            "score": 100.0
        })),
    )).await.unwrap();

    // Direct version creation should be rejected on accumulative boards
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "player", "score": 100.0 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("accumulative"));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn accumulative_snapshot_no_scores_returns_400() {
    let schema = "test_accum_empty_snap";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "scored",
            "accumulative": true
        })),
    )).await.unwrap();

    // Snapshot with no scores should fail
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/snapshot",
        Some(serde_json::json!({})),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("no accumulated scores"));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn accumulative_multiple_snapshots() {
    let schema = "test_accum_multi_snap";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "scored",
            "accumulative": true
        })),
    )).await.unwrap();

    // Submit scores
    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/scores",
        Some(serde_json::json!({
            "entry_slug": "a",
            "entry_name": "A",
            "score": 100.0
        })),
    )).await.unwrap();

    // First snapshot
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/snapshot",
        Some(serde_json::json!({ "note": "snap 1" })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let v1 = json_body(resp).await;
    assert_eq!(v1["version_number"], 1);

    // Update score and add new player
    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/scores",
        Some(serde_json::json!({
            "entry_slug": "a",
            "entry_name": "A",
            "score": 200.0
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/scores",
        Some(serde_json::json!({
            "entry_slug": "b",
            "entry_name": "B",
            "score": 150.0
        })),
    )).await.unwrap();

    // Second snapshot — scores persist (not cleared)
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards/board/snapshot",
        Some(serde_json::json!({ "note": "snap 2" })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let v2 = json_body(resp).await;
    assert_eq!(v2["version_number"], 2);
    let placements = v2["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 2);
    // A has updated score of 200
    assert_eq!(placements[0]["entry_slug"], "a");
    assert_eq!(placements[0]["score"], 200.0);
    assert_eq!(placements[0]["position"], 1);
    assert_eq!(placements[1]["entry_slug"], "b");
    assert_eq!(placements[1]["score"], 150.0);

    // Version listing should show both
    let resp = app.clone().oneshot(json_request(
        "GET",
        "/boards/board/versions",
        None,
    )).await.unwrap();
    let versions = json_body(resp).await;
    assert_eq!(versions["items"].as_array().unwrap().len(), 2);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn accumulative_board_delete_cascades() {
    let schema = "test_accum_cascade";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "scored",
            "accumulative": true
        })),
    )).await.unwrap();

    app.clone().oneshot(json_request(
        "POST",
        "/boards/board/scores",
        Some(serde_json::json!({
            "entry_slug": "player",
            "entry_name": "Player",
            "score": 100.0
        })),
    )).await.unwrap();

    // Delete board — accumulated_scores should cascade
    let resp = app.clone().oneshot(json_request("DELETE", "/boards/board", None)).await.unwrap();
    assert_eq!(resp.status(), 204);

    let resp = app.clone().oneshot(json_request("GET", "/boards/board", None)).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn accumulative_non_scored_board_rejected() {
    let schema = "test_accum_non_scored";
    let (state, app) = setup(schema).await;

    // Accumulative ordered board should be rejected
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered",
            "accumulative": true
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("accumulative boards must have board_type 'scored'"));

    // Accumulative tiered board should also be rejected
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "board2",
            "name": "Board2",
            "board_type": "tiered",
            "accumulative": true
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Phase 6: File Sync ───────────────────────────────────────────────────

async fn setup_with_sync(schema: &str) -> (Arc<AppState>, axum::Router) {
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool,
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let router = build_router(state.clone());
    (state, router)
}

fn create_temp_boards_dir() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("failed to create temp dir")
}

fn write_board_files(
    dir: &std::path::Path,
    slug: &str,
    board_toml: &str,
    rankings_toml: Option<&str>,
) {
    let board_dir = dir.join(slug);
    std::fs::create_dir_all(&board_dir).expect("failed to create board dir");
    std::fs::write(board_dir.join("board.toml"), board_toml).expect("failed to write board.toml");
    if let Some(rankings) = rankings_toml {
        std::fs::write(board_dir.join("rankings.toml"), rankings)
            .expect("failed to write rankings.toml");
    }
}

#[tokio::test]
async fn sync_ordered_board_creates_version() {
    let schema = "test_sync_ordered";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    write_board_files(
        tmp.path(),
        "dc-sandwiches",
        r#"
name = "Best Sandwiches in DC"
board_type = "ordered"
"#,
        Some(r#"
[[entries]]
slug = "crunchy-boi"
name = "Compliments Only Crunchy Boi"
position = 1

[[entries]]
slug = "humberto"
name = "Dupont Market Humberto"
position = 2

[[entries]]
slug = "a-litteri"
name = "A. Litteri Italian"
position = 3
"#),
    );

    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        Some("Initial sync"),
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].slug, "dc-sandwiches");
    assert!(matches!(
        results[0].action,
        riley_leaderboards_core::sync::execute::SyncAction::Created { version_number: 1 }
    ));

    // Verify the board exists
    let board = riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, "dc-sandwiches")
        .await
        .unwrap();
    assert_eq!(board.name, "Best Sandwiches in DC");
    assert_eq!(board.board_type, "ordered");

    // Verify version has correct placements
    let version =
        riley_leaderboards_core::repo::versions::get_latest(&state.pool, board.id).await.unwrap();
    assert_eq!(version.version.version_number, 1);
    assert_eq!(version.placements.len(), 3);
    assert_eq!(version.placements[0].entry_slug, "crunchy-boi");
    assert_eq!(version.placements[0].position, Some(1));
    assert_eq!(version.placements[1].entry_slug, "humberto");
    assert_eq!(version.placements[2].entry_slug, "a-litteri");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn sync_tiered_board_creates_version() {
    let schema = "test_sync_tiered";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    write_board_files(
        tmp.path(),
        "nfl-draft",
        r#"
name = "2026 NFL Draft Prospects"
board_type = "tiered"

[[tiers]]
key = "elite"
label = "Elite (Top 5 Pick)"

[[tiers]]
key = "first_round"
label = "First Round"
"#,
        Some(r#"
[[entries]]
slug = "travis-hunter"
name = "Travis Hunter"
tier = "elite"
position = 1

[[entries]]
slug = "cam-ward"
name = "Cam Ward"
tier = "elite"
position = 2

[[entries]]
slug = "tetairoa-mcmillan"
name = "Tetairoa McMillan"
tier = "first_round"
position = 1
"#),
    );

    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        None,
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    assert!(matches!(
        results[0].action,
        riley_leaderboards_core::sync::execute::SyncAction::Created { .. }
    ));

    let board = riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, "nfl-draft")
        .await
        .unwrap();
    assert_eq!(board.board_type, "tiered");
    assert!(board.tier_config.is_some());

    let version =
        riley_leaderboards_core::repo::versions::get_latest(&state.pool, board.id).await.unwrap();
    assert_eq!(version.placements.len(), 3);
    assert_eq!(version.placements[0].entry_slug, "travis-hunter");
    assert_eq!(version.placements[0].tier.as_deref(), Some("elite"));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn sync_scored_board_creates_version() {
    let schema = "test_sync_scored";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    write_board_files(
        tmp.path(),
        "prog-langs",
        r#"
name = "Best Programming Languages"
board_type = "scored"
sort_direction = "desc"
"#,
        Some(r#"
[[entries]]
slug = "rust"
name = "Rust"
score = 95.0

[[entries]]
slug = "python"
name = "Python"
score = 88.0

[[entries]]
slug = "go"
name = "Go"
score = 82.0
"#),
    );

    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        None,
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    let board = riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, "prog-langs")
        .await
        .unwrap();
    let version =
        riley_leaderboards_core::repo::versions::get_latest(&state.pool, board.id).await.unwrap();
    assert_eq!(version.placements.len(), 3);
    // Positions derived: Rust #1 (95), Python #2 (88), Go #3 (82)
    assert_eq!(version.placements[0].entry_slug, "rust");
    assert_eq!(version.placements[0].position, Some(1));
    assert_eq!(version.placements[1].entry_slug, "python");
    assert_eq!(version.placements[1].position, Some(2));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn sync_no_change_skips_version() {
    let schema = "test_sync_noop";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    let board_toml = r#"
name = "Board"
board_type = "ordered"
"#;
    let rankings_toml = r#"
[[entries]]
slug = "alpha"
name = "Alpha"
position = 1

[[entries]]
slug = "beta"
name = "Beta"
position = 2
"#;

    write_board_files(tmp.path(), "board", board_toml, Some(rankings_toml));

    // First sync — creates version 1
    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        results[0].action,
        riley_leaderboards_core::sync::execute::SyncAction::Created { version_number: 1 }
    ));

    // Second sync with same content — should be NoChange
    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        results[0].action,
        riley_leaderboards_core::sync::execute::SyncAction::NoChange
    ));

    // Verify still only version 1
    let board = riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, "board")
        .await
        .unwrap();
    let versions = riley_leaderboards_core::repo::versions::list(&state.pool, board.id)
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn sync_change_creates_new_version() {
    let schema = "test_sync_update";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    let board_toml = r#"
name = "Board"
board_type = "ordered"
"#;

    write_board_files(
        tmp.path(),
        "board",
        board_toml,
        Some(r#"
[[entries]]
slug = "alpha"
name = "Alpha"
position = 1

[[entries]]
slug = "beta"
name = "Beta"
position = 2
"#),
    );

    // First sync
    riley_leaderboards_core::sync::execute::sync_dir(&state.pool, tmp.path(), None)
        .await
        .unwrap();

    // Update rankings: swap positions
    write_board_files(
        tmp.path(),
        "board",
        board_toml,
        Some(r#"
[[entries]]
slug = "beta"
name = "Beta"
position = 1

[[entries]]
slug = "alpha"
name = "Alpha"
position = 2
"#),
    );

    // Second sync
    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        Some("Swapped positions"),
    )
    .await
    .unwrap();
    assert!(matches!(
        results[0].action,
        riley_leaderboards_core::sync::execute::SyncAction::Updated { version_number: 2 }
    ));

    // Verify version 2 has new order
    let board = riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, "board")
        .await
        .unwrap();
    let version =
        riley_leaderboards_core::repo::versions::get_latest(&state.pool, board.id).await.unwrap();
    assert_eq!(version.version.version_number, 2);
    assert_eq!(version.placements[0].entry_slug, "beta");
    assert_eq!(version.placements[0].position, Some(1));
    assert_eq!(version.version.note.as_deref(), Some("Swapped positions"));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn sync_accumulative_board_skipped() {
    let schema = "test_sync_skip_accum";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    write_board_files(
        tmp.path(),
        "game-scores",
        r#"
name = "Game Scores"
board_type = "scored"
accumulative = true
"#,
        Some(r#"
[[entries]]
slug = "player1"
name = "Player 1"
score = 100.0
"#),
    );

    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        None,
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    assert!(matches!(
        results[0].action,
        riley_leaderboards_core::sync::execute::SyncAction::Skipped { .. }
    ));

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn sync_board_metadata_from_toml() {
    let schema = "test_sync_metadata";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    write_board_files(
        tmp.path(),
        "sandwiches",
        r#"
name = "Best Sandwiches"
board_type = "ordered"

[metadata]
description = "A definitive ranking."
author = "Riley"
"#,
        Some(r#"
[[entries]]
slug = "crunchy-boi"
name = "Crunchy Boi"
position = 1

[entries.metadata]
address = "1026 Vermont Ave NW"
image_url = "https://example.com/crunchy.jpg"
"#),
    );

    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);

    let board = riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, "sandwiches")
        .await
        .unwrap();
    let meta = board.metadata.unwrap();
    assert_eq!(meta["description"], "A definitive ranking.");
    assert_eq!(meta["author"], "Riley");

    let version =
        riley_leaderboards_core::repo::versions::get_latest(&state.pool, board.id).await.unwrap();
    let entry_meta = version.placements[0].metadata.as_ref().unwrap();
    assert_eq!(entry_meta["address"], "1026 Vermont Ave NW");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn sync_multiple_boards() {
    let schema = "test_sync_multi";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    write_board_files(
        tmp.path(),
        "board-a",
        r#"
name = "Board A"
board_type = "ordered"
"#,
        Some(r#"
[[entries]]
slug = "item"
name = "Item"
position = 1
"#),
    );

    write_board_files(
        tmp.path(),
        "board-b",
        r#"
name = "Board B"
board_type = "ordered"
"#,
        Some(r#"
[[entries]]
slug = "item"
name = "Item"
position = 1
"#),
    );

    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        None,
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 2);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn webhook_missing_signature_returns_400() {
    let schema = "test_webhook_no_sig";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: Some(riley_leaderboards_core::config::SyncConfig {
            repo_path: Some("/tmp/nonexistent".to_string()),
            webhook_secret: Some(ConfigValue::new("test-secret")),
            sync_branch: None,
        }),
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/github")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"ref": "refs/heads/main"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body = json_body(resp).await;
    assert!(body["error"].as_str().unwrap().contains("missing"));

    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn webhook_invalid_signature_returns_401() {
    let schema = "test_webhook_bad_sig";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: Some(riley_leaderboards_core::config::SyncConfig {
            repo_path: Some("/tmp/nonexistent".to_string()),
            webhook_secret: Some(ConfigValue::new("test-secret")),
            sync_branch: None,
        }),
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/github")
                .header("content-type", "application/json")
                .header("x-hub-signature-256", "sha256=deadbeef")
                .body(Body::from(r#"{"ref": "refs/heads/main"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn webhook_valid_signature_triggers_sync() {
    let schema = "test_webhook_valid";

    // Set up a proper git repo so the webhook's git fetch+reset succeeds.
    // Create a bare repo, clone it, add board files, commit, and push.
    let bare_dir = tempfile::tempdir().unwrap();
    let work_dir = tempfile::tempdir().unwrap();

    // Init bare repo
    std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(bare_dir.path())
        .output()
        .unwrap();

    // Clone it to work_dir
    std::process::Command::new("git")
        .args([
            "clone",
            &bare_dir.path().to_string_lossy(),
            &work_dir.path().to_string_lossy(),
        ])
        .output()
        .unwrap();

    // Add board files
    write_board_files(
        work_dir.path(),
        "webhook-board",
        r#"
name = "Webhook Board"
board_type = "ordered"
"#,
        Some(r#"
[[entries]]
slug = "item"
name = "Item"
position = 1
"#),
    );

    // Commit and push
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(work_dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(work_dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["push"])
        .current_dir(work_dir.path())
        .output()
        .unwrap();

    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: Some(riley_leaderboards_core::config::SyncConfig {
            repo_path: Some(work_dir.path().to_string_lossy().to_string()),
            webhook_secret: Some(ConfigValue::new("test-secret")),
            sync_branch: None,
        }),
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    // Compute valid HMAC signature
    let body_bytes = serde_json::to_vec(&serde_json::json!({
        "ref": "refs/heads/main",
        "head_commit": {
            "message": "Update rankings"
        }
    }))
    .unwrap();

    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(b"test-secret").unwrap();
    mac.update(&body_bytes);
    let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/github")
                .header("content-type", "application/json")
                .header("x-hub-signature-256", &signature)
                .header("x-github-event", "push")
                .body(Body::from(body_bytes))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert!(body["synced"].is_array());

    // Verify the board was created
    let board = riley_leaderboards_core::repo::boards::get_by_slug(&pool, "webhook-board")
        .await
        .unwrap();
    let version = riley_leaderboards_core::repo::versions::get_latest(&pool, board.id)
        .await
        .unwrap();
    assert_eq!(version.version.version_number, 1);
    assert_eq!(version.version.note.as_deref(), Some("Update rankings"));

    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

// ── Phase 7: Auth ───────────────────────────────────────────────────────

const TEST_RSA_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCNwoHDhRndaP6w
+w2K7a0z2TuLBWzoVe6A4A00QpzpiautWCTnQSfTHAU343GvCGEyDe8sNVUIOLqR
+0h+zLEYb0yu1+PYnz8G4J1SGPec8U0wy9w6pGnA/eE0c5qfUzVatO4rxdp0NmPo
Zk0ZZa3TJc+gKSICij7Z4N1d5Mux3/FLZO/gE/WPctFANJFitGU+VZgPVvrfcpc5
joetYPu2LUHgN7deKbEnLXRbffqI6gJFSVM04prboBAyI/s6VekMChkKW9YiKjGf
Sf9kfapIQjiT6GtSt82futLB6Ll7qBFtw6+bBKFaRe7qmrf+UikyQfC15iMXXp0L
OIHscJ09AgMBAAECggEAAR4B0M0pPYX4z+NCoZAq98gkAH379D7NIOXjJMDLpMmJ
eVXDALGSQ0cqwVyBBlyeC3txoZsP/v8XdVQSJ7GsSaGC7LPV31yt7ftyMfXxaaK7
NYG9zBaEoNk/X57znoLU3lCjueOWy6isE+ZOgD89Zfcb0krQsk1tnmD3zagidNXo
e7If+oqEd9a7gRPhhbE8kwFbt/7JmxqcjqM4sr/icMPHHiI3MFNqLZgM2gMcW8IR
6/3jGMOQtRxV7aT+7mGKqVLfzYloPblAp380nWPDZsoOf+rZAskr2qCYPo2SVqyG
ntz6+Fw1/4lod15UNxCYdW6+MmFrEXRibTuIJ/4UYQKBgQDAF4/Iqm8BENvjnwMe
GyBtrBNHLf/dT/vc5nUTxnMHXGhZ0MqgX/ILRoRk9g99oA4T2Bv/fpGjyvtgPTQz
fPHy20M9zorX64t/hTyYcGEtWMvzIksl2gn9NaN0TNFqK8jf7f5JnU3SH7xv+yxk
uN3vDnItjwjb6kEXiPKO/deNbQKBgQC87CiVKQHiOhdgtpARmflxIcgHWYfi4WKT
C5avJVbv0N0A4uacTPkx5IMrnfQgyOzxHzU2pGSOpRE2yB5VTqCoMYCo+E4BXsUr
KbwPrE2pBYn0K83hSQHBpRkq1lQUdN1QCUMgbqGRJoKOKcVE80VgidBxD8KJ50DF
Y5BZxBp9EQKBgEDvNhG1W3TWyB44AIvKy7mHM7UaHaYohZF07hrTOMtCN5w08moo
RN/+5H5kl3P2CQw4P66skHr4AOXVirHlCLz51c8s5M58t1lSJtu5EYCMxdTYwOJ4
xGuuGCUWWqwzROI9x3oHDOl9BOwt0iHyREOtdHdmJK6Cj6JvDt+7e4Q5AoGBALBb
W/7xytpeFBiqE476x0n+mPWTdDAs6ZIOzVkuaBtyQ/xh05iwmicjA/eheZVpOxZT
ZZ9ekqg+GvWilf5YaczYeRxCvr60syX5zZ5r4AsaKo+OnJ/jQQp9jiLY9KAr/7SJ
EOqjm5sd8d23zHjzBx55R+VjKt0EzQf2S3ggggGhAoGAb0vcYtuh/vjY+3AqUAsw
WZGgVBrlFZQrCBD91A/e/pST2dd/TQBbofW0a8GXYGhR8CwLZ7GJzgdmfyHaOSXN
9K6tQ87hWREFWgBVel3+wl4U3WPb4NFp0Jt/sE2g+gHbCGt+BooaGk+fqlTpRlsD
2NOpUy2KL4uigc/vwiB0qTQ=
-----END PRIVATE KEY-----";

const TEST_RSA_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAjcKBw4UZ3Wj+sPsNiu2t
M9k7iwVs6FXugOANNEKc6YmrrVgk50En0xwFN+NxrwhhMg3vLDVVCDi6kftIfsyx
GG9Mrtfj2J8/BuCdUhj3nPFNMMvcOqRpwP3hNHOan1M1WrTuK8XadDZj6GZNGWWt
0yXPoCkiAoo+2eDdXeTLsd/xS2Tv4BP1j3LRQDSRYrRlPlWYD1b633KXOY6HrWD7
ti1B4De3XimxJy10W336iOoCRUlTNOKa26AQMiP7OlXpDAoZClvWIioxn0n/ZH2q
SEI4k+hrUrfNn7rSwei5e6gRbcOvmwShWkXu6pq3/lIpMkHwteYjF16dCziB7HCd
PQIDAQAB
-----END PUBLIC KEY-----";

fn make_test_jwt(roles: &[&str], exp_offset_secs: i64) -> String {
    use jsonwebtoken::{Algorithm, EncodingKey, Header};

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key-1".to_string());

    let now = chrono::Utc::now().timestamp();
    let claims = serde_json::json!({
        "sub": "test-user",
        "roles": roles,
        "iat": now,
        "exp": now + exp_offset_secs,
    });

    let key = EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY.as_bytes()).unwrap();
    jsonwebtoken::encode(&header, &claims, &key).unwrap()
}

fn setup_jwt_auth_state(
    pool: sqlx::PgPool,
    config: RileyLeaderboardsConfig,
    required_role: Option<String>,
) -> (Arc<AppState>, axum::Router) {
    let decoding_key =
        jsonwebtoken::DecodingKey::from_rsa_pem(TEST_RSA_PUBLIC_KEY.as_bytes()).unwrap();
    let jwks_cache = riley_leaderboards_api::auth::JwksCache::from_static(
        "test-key-1".to_string(),
        decoding_key,
        jsonwebtoken::Algorithm::RS256,
    );
    let auth_mode = riley_leaderboards_api::auth::AuthMode::Jwt {
        jwks_cache: Arc::new(jwks_cache),
        required_role,
        read_token_hashes: vec![],
        require_read_auth: false,
    };
    let state = Arc::new(AppState {
        pool,
        config,
        auth_mode,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let router = build_router(state.clone());
    (state, router)
}

#[tokio::test]
async fn auth_api_token_write_requires_token() {
    let schema = "test_auth_api_token";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    // Set up with API token auth
    let token_hash = {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let mut mac =
            Hmac::<Sha256>::new_from_slice(b"riley-leaderboards-api-token").unwrap();
        mac.update(b"test-api-secret-123");
        mac.finalize().into_bytes().to_vec()
    };
    let auth_mode = riley_leaderboards_api::auth::AuthMode::ApiToken {
        admin_token_hash: token_hash,
        read_token_hashes: vec![],
        require_read_auth: false,
    };
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    // POST without token → 401
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": "test-board",
                "name": "Test Board",
                "board_type": "ordered"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // POST with wrong token → 401
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", "Bearer wrong-token")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "test-board",
                        "name": "Test Board",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // POST with valid token → 201
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-api-secret-123")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "test-board",
                        "name": "Test Board",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // GET without token → 200 (reads are public)
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn auth_jwt_valid_token_allows_write() {
    let schema = "test_auth_jwt_valid";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let (state, app) =
        setup_jwt_auth_state(pool.clone(), config, Some("admin".to_string()));

    let token = make_test_jwt(&["admin"], 3600);

    // POST with valid JWT → 201
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "jwt-board",
                        "name": "JWT Board",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // GET without JWT → 200 (reads are public)
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards/jwt-board")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn auth_jwt_expired_token_rejected() {
    let schema = "test_auth_jwt_expired";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let (_state, app) =
        setup_jwt_auth_state(pool.clone(), config, Some("admin".to_string()));

    // Create an expired JWT (exp = 1 second ago)
    // Offset must exceed jsonwebtoken's 60-second leeway
    let token = make_test_jwt(&["admin"], -120);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "expired-board",
                        "name": "Expired Board",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn auth_jwt_wrong_role_rejected() {
    let schema = "test_auth_jwt_role";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let (_state, app) =
        setup_jwt_auth_state(pool.clone(), config, Some("admin".to_string()));

    // Create a JWT with the wrong role
    let token = make_test_jwt(&["viewer"], 3600);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "wrong-role-board",
                        "name": "Wrong Role Board",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let body = json_body(resp).await;
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("insufficient permissions"));

    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn auth_jwt_no_required_role_any_jwt_passes() {
    let schema = "test_auth_jwt_no_role";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    // No required_role — any valid JWT should pass
    let (state, app) = setup_jwt_auth_state(pool.clone(), config, None);

    let token = make_test_jwt(&[], 3600);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "no-role-board",
                        "name": "No Role Board",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn auth_no_auth_mode_allows_everything() {
    let schema = "test_auth_no_auth";
    let (state, app) = setup(schema).await;

    // POST without any auth → 201 (no-auth mode)
    let resp = app
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": "open-board",
                "name": "Open Board",
                "board_type": "ordered"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn auth_jwt_missing_token_returns_401() {
    let schema = "test_auth_jwt_missing";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let (_state, app) =
        setup_jwt_auth_state(pool.clone(), config, Some("admin".to_string()));

    // POST without Authorization header → 401
    let resp = app
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": "missing-token-board",
                "name": "Missing Token",
                "board_type": "ordered"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let body = json_body(resp).await;
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("missing"));

    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn auth_jwks_fetch_from_mock_server() {
    let schema = "test_auth_jwks_fetch";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    // Start a mock JWKS server
    let jwks_json = serde_json::json!({
        "keys": [{
            "kty": "RSA",
            "kid": "test-key-1",
            "use": "sig",
            "alg": "RS256",
            "n": "jcKBw4UZ3Wj-sPsNiu2tM9k7iwVs6FXugOANNEKc6YmrrVgk50En0xwFN-NxrwhhMg3vLDVVCDi6kftIfsyxGG9Mrtfj2J8_BuCdUhj3nPFNMMvcOqRpwP3hNHOan1M1WrTuK8XadDZj6GZNGWWt0yXPoCkiAoo-2eDdXeTLsd_xS2Tv4BP1j3LRQDSRYrRlPlWYD1b633KXOY6HrWD7ti1B4De3XimxJy10W336iOoCRUlTNOKa26AQMiP7OlXpDAoZClvWIioxn0n_ZH2qSEI4k-hrUrfNn7rSwei5e6gRbcOvmwShWkXu6pq3_lIpMkHwteYjF16dCziB7HCdPQ",
            "e": "AQAB"
        }]
    });
    let jwks_handler = {
        let jwks = jwks_json.clone();
        move || {
            let jwks = jwks.clone();
            async move { axum::Json(jwks) }
        }
    };
    let mock_app = axum::Router::new().route(
        "/.well-known/jwks.json",
        axum::routing::get(jwks_handler),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let jwks_url = format!("http://{mock_addr}/.well-known/jwks.json");

    // Create JwksCache from real HTTP fetch
    let jwks_cache =
        riley_leaderboards_api::auth::JwksCache::new(&jwks_url).await.unwrap();
    let jwks_cache = Arc::new(jwks_cache);

    let auth_mode = riley_leaderboards_api::auth::AuthMode::Jwt {
        jwks_cache,
        required_role: Some("admin".to_string()),
        read_token_hashes: vec![],
        require_read_auth: false,
    };
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    // Sign a JWT with the test private key
    let token = make_test_jwt(&["admin"], 3600);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "jwks-board",
                        "name": "JWKS Board",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

// ── Phase 8: Pagination + Export/Import ─────────────────────────────────

#[tokio::test]
async fn pagination_boards_list() {
    let schema = "test_pagination_boards";
    let (state, app) = setup(schema).await;

    // Create 3 boards
    for i in 0..3 {
        let resp = app.clone().oneshot(json_request(
            "POST", "/boards",
            Some(serde_json::json!({
                "slug": format!("page-b{i}"),
                "name": format!("Page Board {i}"),
                "board_type": "ordered"
            })),
        )).await.unwrap();
        assert_eq!(resp.status(), 201, "create board {i}");
    }

    // Fetch with limit=2
    let resp = app.clone().oneshot(json_request("GET", "/boards?limit=2", None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "first page should have 2 items");
    assert!(body["next_cursor"].as_str().is_some(), "should have a next_cursor");

    // Fetch second page using cursor
    let cursor = body["next_cursor"].as_str().unwrap();
    let encoded_cursor = urlencoding::encode(cursor);
    let uri = format!("/boards?limit=2&cursor={encoded_cursor}");
    let resp = app.clone().oneshot(json_request("GET", &uri, None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body2 = json_body(resp).await;
    let items2 = body2["items"].as_array().unwrap();
    assert_eq!(items2.len(), 1, "second page should have 1 item");
    assert!(body2["next_cursor"].is_null(), "no more pages");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn pagination_entries_list() {
    let schema = "test_pagination_entries";
    let (state, app) = setup(schema).await;

    // Create board
    let resp = app.clone().oneshot(json_request(
        "POST", "/boards",
        Some(serde_json::json!({
            "slug": "paged",
            "name": "Paged Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Create 3 entries
    for i in 0..3 {
        let resp = app.clone().oneshot(json_request(
            "POST", "/boards/paged/entries",
            Some(serde_json::json!({
                "slug": format!("e{i}"),
                "name": format!("Entry {i}")
            })),
        )).await.unwrap();
        assert_eq!(resp.status(), 201);
    }

    // Fetch with limit=1
    let resp = app.clone().oneshot(json_request("GET", "/boards/paged/entries?limit=1", None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert!(body["next_cursor"].as_str().is_some());

    // Fetch second page
    let cursor = body["next_cursor"].as_str().unwrap();
    let encoded_cursor = urlencoding::encode(cursor);
    let uri = format!("/boards/paged/entries?limit=1&cursor={encoded_cursor}");
    let resp = app.clone().oneshot(json_request("GET", &uri, None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body2 = json_body(resp).await;
    let items2 = body2["items"].as_array().unwrap();
    assert_eq!(items2.len(), 1);
    assert!(body2["next_cursor"].as_str().is_some());

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn export_import_round_trip() {
    let schema = "test_export_import";
    let (state, app) = setup(schema).await;

    // Create board with entries and a version
    let resp = app.clone().oneshot(json_request(
        "POST", "/boards",
        Some(serde_json::json!({
            "slug": "export-test",
            "name": "Export Test",
            "board_type": "scored",
            "sort_direction": "desc"
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);

    for i in 0..2 {
        let resp = app.clone().oneshot(json_request(
            "POST", "/boards/export-test/entries",
            Some(serde_json::json!({
                "slug": format!("exp-e{i}"),
                "name": format!("Export Entry {i}")
            })),
        )).await.unwrap();
        assert_eq!(resp.status(), 201);
    }

    // Create a version with placements
    let resp = app.clone().oneshot(json_request(
        "POST", "/boards/export-test/versions",
        Some(serde_json::json!({
            "placements": [
                { "entry_slug": "exp-e0", "score": 100.0, "position": 1 },
                { "entry_slug": "exp-e1", "score": 200.0, "position": 2 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Export
    let export = riley_leaderboards_core::repo::export::export_board(
        &state.pool, "export-test",
    ).await.expect("export failed");
    assert_eq!(export.board.slug, "export-test");
    assert_eq!(export.versions.len(), 1);
    assert_eq!(export.versions[0].placements.len(), 2);

    // Roundtrip: serialize → deserialize
    let json_str = serde_json::to_string_pretty(&export).unwrap();
    let reimported: riley_leaderboards_core::repo::export::BoardExport =
        serde_json::from_str(&json_str).unwrap();
    assert_eq!(reimported.board.slug, "export-test");

    // Delete the original board
    let resp = app.clone().oneshot(json_request(
        "DELETE", "/boards/export-test", None,
    )).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Import from the deserialized export
    riley_leaderboards_core::repo::export::import_board(&state.pool, &reimported)
        .await.expect("import failed");

    // Verify the board exists again
    let resp = app.clone().oneshot(json_request(
        "GET", "/boards/export-test", None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["slug"], "export-test");
    assert_eq!(body["board_type"], "scored");

    // Verify version was imported
    let resp = app.clone().oneshot(json_request(
        "GET", "/boards/export-test/versions/1", None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);

    cleanup(&state, schema).await;
}

// ── Version Metadata ─────────────────────────────────────────────────────

#[tokio::test]
async fn version_metadata_create_and_fetch() {
    let schema = "test_version_metadata";
    let (state, app) = setup(schema).await;

    // Create board + entry
    app.clone().oneshot(json_request(
        "POST", "/boards",
        Some(serde_json::json!({
            "slug": "meta-board",
            "name": "Metadata Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    app.clone().oneshot(json_request(
        "POST", "/boards/meta-board/entries",
        Some(serde_json::json!({ "slug": "item-a", "name": "Item A" })),
    )).await.unwrap();

    // Create version WITH metadata
    let resp = app.clone().oneshot(json_request(
        "POST", "/boards/meta-board/versions",
        Some(serde_json::json!({
            "note": "February update",
            "metadata": {
                "blog_post_url": "https://example.com/blog/feb-update",
                "changelog": "Added Item A"
            },
            "placements": [
                { "entry_slug": "item-a", "position": 1 }
            ]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    assert_eq!(version["version_number"], 1);
    assert_eq!(version["metadata"]["blog_post_url"], "https://example.com/blog/feb-update");
    assert_eq!(version["metadata"]["changelog"], "Added Item A");

    // Fetch by number — metadata persists
    let resp = app.clone().oneshot(json_request(
        "GET", "/boards/meta-board/versions/1", None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let fetched = json_body(resp).await;
    assert_eq!(fetched["metadata"]["blog_post_url"], "https://example.com/blog/feb-update");

    // Fetch latest — metadata persists
    let resp = app.clone().oneshot(json_request(
        "GET", "/boards/meta-board/latest", None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let latest = json_body(resp).await;
    assert_eq!(latest["metadata"]["changelog"], "Added Item A");

    // List versions — metadata visible
    let resp = app.clone().oneshot(json_request(
        "GET", "/boards/meta-board/versions", None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let list = json_body(resp).await;
    assert_eq!(list["items"][0]["metadata"]["blog_post_url"], "https://example.com/blog/feb-update");

    // Since endpoint — metadata visible
    let resp = app.clone().oneshot(json_request(
        "GET", "/boards/meta-board/since/0", None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let since = json_body(resp).await;
    assert_eq!(since[0]["metadata"]["blog_post_url"], "https://example.com/blog/feb-update");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn version_metadata_null_is_fine() {
    let schema = "test_version_meta_null";
    let (state, app) = setup(schema).await;

    // Create board + entry
    app.clone().oneshot(json_request(
        "POST", "/boards",
        Some(serde_json::json!({
            "slug": "board",
            "name": "Board",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    app.clone().oneshot(json_request(
        "POST", "/boards/board/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();

    // Create version WITHOUT metadata
    let resp = app.clone().oneshot(json_request(
        "POST", "/boards/board/versions",
        Some(serde_json::json!({
            "note": "No metadata",
            "placements": [{ "entry_slug": "item", "position": 1 }]
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    assert!(version["metadata"].is_null());

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn snapshot_with_metadata() {
    let schema = "test_snapshot_metadata";
    let (state, app) = setup(schema).await;

    // Create accumulative board
    app.clone().oneshot(json_request(
        "POST", "/boards",
        Some(serde_json::json!({
            "slug": "game",
            "name": "Game Scores",
            "board_type": "scored",
            "accumulative": true
        })),
    )).await.unwrap();

    // Submit score
    app.clone().oneshot(json_request(
        "POST", "/boards/game/scores",
        Some(serde_json::json!({
            "entry_slug": "player1",
            "entry_name": "Player 1",
            "score": 100.0
        })),
    )).await.unwrap();

    // Snapshot WITH metadata
    let resp = app.clone().oneshot(json_request(
        "POST", "/boards/game/snapshot",
        Some(serde_json::json!({
            "note": "End of round 1",
            "metadata": {
                "round": 1,
                "tournament": "Summer 2026"
            }
        })),
    )).await.unwrap();
    assert_eq!(resp.status(), 201);
    let version = json_body(resp).await;
    assert_eq!(version["metadata"]["round"], 1);
    assert_eq!(version["metadata"]["tournament"], "Summer 2026");

    // Fetch latest — metadata persists
    let resp = app.clone().oneshot(json_request(
        "GET", "/boards/game/latest", None,
    )).await.unwrap();
    let latest = json_body(resp).await;
    assert_eq!(latest["metadata"]["tournament"], "Summer 2026");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn export_import_preserves_version_metadata() {
    let schema = "test_export_version_meta";
    let (state, app) = setup(schema).await;

    // Create board + entry + version with metadata
    app.clone().oneshot(json_request(
        "POST", "/boards",
        Some(serde_json::json!({
            "slug": "export-meta",
            "name": "Export Meta Test",
            "board_type": "ordered"
        })),
    )).await.unwrap();
    app.clone().oneshot(json_request(
        "POST", "/boards/export-meta/entries",
        Some(serde_json::json!({ "slug": "item", "name": "Item" })),
    )).await.unwrap();
    app.clone().oneshot(json_request(
        "POST", "/boards/export-meta/versions",
        Some(serde_json::json!({
            "note": "v1",
            "metadata": { "source": "test", "url": "https://example.com" },
            "placements": [{ "entry_slug": "item", "position": 1 }]
        })),
    )).await.unwrap();

    // Export
    use riley_leaderboards_core::repo::export;
    let exported = export::export_board(&state.pool, "export-meta").await.unwrap();
    assert_eq!(exported.versions[0].metadata.as_ref().unwrap()["source"], "test");

    // Delete the board, then re-import
    app.clone().oneshot(json_request(
        "DELETE", "/boards/export-meta", None,
    )).await.unwrap();

    export::import_board(&state.pool, &exported).await.unwrap();

    // Verify metadata survived the round-trip
    let resp = app.clone().oneshot(json_request(
        "GET", "/boards/export-meta/versions/1", None,
    )).await.unwrap();
    assert_eq!(resp.status(), 200);
    let version = json_body(resp).await;
    assert_eq!(version["metadata"]["source"], "test");
    assert_eq!(version["metadata"]["url"], "https://example.com");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn sync_with_version_metadata() {
    let schema = "test_sync_version_meta";
    let (state, _app) = setup_with_sync(schema).await;
    let tmp = create_temp_boards_dir();

    write_board_files(
        tmp.path(),
        "meta-sync",
        r#"
name = "Meta Sync Test"
board_type = "ordered"
"#,
        Some(r#"
[version_metadata]
blog_post_url = "https://example.com/post"
author = "test"

[[entries]]
slug = "item-a"
name = "Item A"
position = 1
"#),
    );

    let results = riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        tmp.path(),
        Some("Sync with metadata"),
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    assert!(matches!(
        results[0].action,
        riley_leaderboards_core::sync::execute::SyncAction::Created { version_number: 1 }
    ));

    // Verify version metadata was stored
    let board = riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, "meta-sync")
        .await
        .unwrap();
    let version =
        riley_leaderboards_core::repo::versions::get_latest(&state.pool, board.id).await.unwrap();
    let meta = version.version.metadata.expect("version metadata should be set");
    assert_eq!(meta["blog_post_url"], "https://example.com/post");
    assert_eq!(meta["author"], "test");

    cleanup(&state, schema).await;
}

// ── Phase 2 (v2): Read-Only API Keys ────────────────────────────────────

/// Helper: compute HMAC-SHA256 hash of a token for test AuthMode construction.
fn hash_test_token(token: &str) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(b"riley-leaderboards-api-token").unwrap();
    mac.update(token.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

#[tokio::test]
async fn auth_read_token_can_read_but_not_write() {
    let schema = "test_read_token_rw";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let admin_token_hash = hash_test_token("admin-secret");
    let read_token_hash = hash_test_token("read-secret");

    let auth_mode = riley_leaderboards_api::auth::AuthMode::ApiToken {
        admin_token_hash,
        read_token_hashes: vec![read_token_hash],
        require_read_auth: false,
    };
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state.clone());

    // First, create a board with the admin token so we have something to read
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", "Bearer admin-secret")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "read-test",
                        "name": "Read Test",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // GET with read-only token → 200
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .header("authorization", "Bearer read-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // POST with read-only token → 401 (writes require admin)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", "Bearer read-secret")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "should-fail",
                        "name": "Should Fail",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // DELETE with read-only token → 401
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/boards/read-test")
                .header("authorization", "Bearer read-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn auth_require_read_auth_blocks_unauthenticated_reads() {
    let schema = "test_require_read_auth";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let admin_token_hash = hash_test_token("admin-secret");
    let read_token_hash = hash_test_token("read-secret");

    let auth_mode = riley_leaderboards_api::auth::AuthMode::ApiToken {
        admin_token_hash,
        read_token_hashes: vec![read_token_hash],
        require_read_auth: true,
    };
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state.clone());

    // Create a board with admin token
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", "Bearer admin-secret")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "auth-read-test",
                        "name": "Auth Read Test",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // GET without token → 401 (require_read_auth is true)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // GET with read-only token → 200
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .header("authorization", "Bearer read-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // GET with admin token → 200
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .header("authorization", "Bearer admin-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // GET with wrong token → 401
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .header("authorization", "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn auth_jwt_read_token_can_read_but_not_write() {
    let schema = "test_jwt_read_token";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let read_token_hash = hash_test_token("jwt-read-secret");

    let decoding_key =
        jsonwebtoken::DecodingKey::from_rsa_pem(TEST_RSA_PUBLIC_KEY.as_bytes()).unwrap();
    let jwks_cache = riley_leaderboards_api::auth::JwksCache::from_static(
        "test-key-1".to_string(),
        decoding_key,
        jsonwebtoken::Algorithm::RS256,
    );
    let auth_mode = riley_leaderboards_api::auth::AuthMode::Jwt {
        jwks_cache: Arc::new(jwks_cache),
        required_role: Some("admin".to_string()),
        read_token_hashes: vec![read_token_hash],
        require_read_auth: false,
    };
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state.clone());

    // Create a board with JWT admin token
    let admin_jwt = make_test_jwt(&["admin"], 3600);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {admin_jwt}"))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "jwt-read-test",
                        "name": "JWT Read Test",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // GET with read-only API token → 200 (read tokens work for reads)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .header("authorization", "Bearer jwt-read-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // POST with read-only API token → 401 (writes require JWT with role)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/boards")
                .header("content-type", "application/json")
                .header("authorization", "Bearer jwt-read-secret")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "slug": "should-fail",
                        "name": "Should Fail",
                        "board_type": "ordered"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // GET without any token → 200 (reads are public, require_read_auth is false)
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn auth_jwt_require_read_auth() {
    let schema = "test_jwt_require_read";
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![],
    };
    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let read_token_hash = hash_test_token("jwt-read-secret");

    let decoding_key =
        jsonwebtoken::DecodingKey::from_rsa_pem(TEST_RSA_PUBLIC_KEY.as_bytes()).unwrap();
    let jwks_cache = riley_leaderboards_api::auth::JwksCache::from_static(
        "test-key-1".to_string(),
        decoding_key,
        jsonwebtoken::Algorithm::RS256,
    );
    let auth_mode = riley_leaderboards_api::auth::AuthMode::Jwt {
        jwks_cache: Arc::new(jwks_cache),
        required_role: Some("admin".to_string()),
        read_token_hashes: vec![read_token_hash],
        require_read_auth: true,
    };
    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state.clone());

    // GET without token → 401 (require_read_auth is true)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // GET with read-only token → 200
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .header("authorization", "Bearer jwt-read-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // GET with valid JWT (no special role needed for reads) → 200
    let viewer_jwt = make_test_jwt(&["viewer"], 3600);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/boards")
                .header("authorization", format!("Bearer {viewer_jwt}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    cleanup(&state, schema).await;
}

// ── Phase 2 (v2): from_config tests ─────────────────────────────────────

#[tokio::test]
async fn auth_from_config_admin_token_and_api_token_mutual_exclusion() {
    use riley_leaderboards_core::config::AuthConfig;

    let config = AuthConfig {
        jwks_url: None,
        required_role: None,
        admin_token: Some(ConfigValue::new("secret-admin")),
        api_token: Some(ConfigValue::new("secret-api")),
        read_tokens: vec![],
        require_read_auth: false,
    };

    let result = riley_leaderboards_api::auth::AuthMode::from_config(Some(&config)).await;
    let err_msg = result.err().expect("expected error").to_string();
    assert!(
        err_msg.contains("mutually exclusive"),
        "expected mutual exclusion error, got: {err_msg}"
    );
}

#[tokio::test]
async fn auth_from_config_legacy_api_token_works() {
    use riley_leaderboards_core::config::AuthConfig;

    let config = AuthConfig {
        jwks_url: None,
        required_role: None,
        admin_token: None,
        api_token: Some(ConfigValue::new("legacy-secret")),
        read_tokens: vec![],
        require_read_auth: false,
    };

    let result = riley_leaderboards_api::auth::AuthMode::from_config(Some(&config)).await;
    let mode = result.expect("from_config should succeed with api_token");
    assert!(
        matches!(mode, riley_leaderboards_api::auth::AuthMode::ApiToken { .. }),
        "expected ApiToken mode"
    );
}

#[tokio::test]
async fn auth_from_config_read_tokens_without_admin_is_error() {
    use riley_leaderboards_core::config::AuthConfig;

    let config = AuthConfig {
        jwks_url: None,
        required_role: None,
        admin_token: None,
        api_token: None,
        read_tokens: vec![ConfigValue::new("read-only-token")],
        require_read_auth: false,
    };

    let result = riley_leaderboards_api::auth::AuthMode::from_config(Some(&config)).await;
    let err_msg = result.err().expect("read_tokens without admin auth should fail").to_string();
    assert!(
        err_msg.contains("no auth mechanism"),
        "expected misconfig error, got: {err_msg}"
    );
}

#[tokio::test]
async fn auth_from_config_require_read_auth_without_admin_is_error() {
    use riley_leaderboards_core::config::AuthConfig;

    let config = AuthConfig {
        jwks_url: None,
        required_role: None,
        admin_token: None,
        api_token: None,
        read_tokens: vec![],
        require_read_auth: true,
    };

    let result = riley_leaderboards_api::auth::AuthMode::from_config(Some(&config)).await;
    assert!(
        result.err().is_some(),
        "require_read_auth without admin auth should fail"
    );
}

#[tokio::test]
async fn auth_from_config_none_is_no_auth() {
    let mode = riley_leaderboards_api::auth::AuthMode::from_config(None)
        .await
        .expect("from_config(None) should succeed");
    assert!(
        matches!(mode, riley_leaderboards_api::auth::AuthMode::NoAuth),
        "expected NoAuth mode"
    );
}

#[tokio::test]
async fn auth_from_config_empty_auth_section_is_no_auth() {
    use riley_leaderboards_core::config::AuthConfig;

    let config = AuthConfig {
        jwks_url: None,
        required_role: None,
        admin_token: None,
        api_token: None,
        read_tokens: vec![],
        require_read_auth: false,
    };

    let mode = riley_leaderboards_api::auth::AuthMode::from_config(Some(&config))
        .await
        .expect("empty [auth] section should produce NoAuth");
    assert!(
        matches!(mode, riley_leaderboards_api::auth::AuthMode::NoAuth),
        "expected NoAuth mode"
    );
}

#[tokio::test]
async fn auth_from_config_required_role_without_auth_mode_is_error() {
    use riley_leaderboards_core::config::AuthConfig;

    let config = AuthConfig {
        jwks_url: None,
        required_role: Some("admin".to_string()),
        admin_token: None,
        api_token: None,
        read_tokens: vec![],
        require_read_auth: false,
    };

    let result = riley_leaderboards_api::auth::AuthMode::from_config(Some(&config)).await;
    assert!(
        result.err().is_some(),
        "required_role without auth mode should fail"
    );
}

#[tokio::test]
async fn auth_from_config_jwks_and_admin_token_mutual_exclusion() {
    use riley_leaderboards_core::config::AuthConfig;

    let config = AuthConfig {
        jwks_url: Some("https://example.com/.well-known/jwks.json".to_string()),
        required_role: None,
        admin_token: Some(ConfigValue::new("secret")),
        api_token: None,
        read_tokens: vec![],
        require_read_auth: false,
    };

    let result = riley_leaderboards_api::auth::AuthMode::from_config(Some(&config)).await;
    let err_msg = result.err().expect("jwks_url + admin_token should fail").to_string();
    assert!(
        err_msg.contains("mutually exclusive"),
        "expected mutual exclusion error, got: {err_msg}"
    );
}

// ── Outbound Webhook Tests ────────────────────────────────────────────────

/// Helper: start a TCP listener on a random port and return (url, receiver).
/// The receiver yields each request body as a String.
async fn start_webhook_receiver() -> (String, tokio::sync::mpsc::Receiver<(String, std::collections::HashMap<String, String>)>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}/webhook");
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { break };
            let tx = tx.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 16384];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let raw = String::from_utf8_lossy(&buf[..n]).to_string();

                // Parse headers and body from raw HTTP
                let mut headers = std::collections::HashMap::new();
                let parts: Vec<&str> = raw.splitn(2, "\r\n\r\n").collect();
                if let Some(header_section) = parts.first() {
                    for line in header_section.lines().skip(1) {
                        if let Some((k, v)) = line.split_once(": ") {
                            headers.insert(k.to_lowercase(), v.to_string());
                        }
                    }
                }
                let body = parts.get(1).unwrap_or(&"").to_string();

                let _ = tx.send((body, headers)).await;

                // Send HTTP 200 response
                let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });

    (url, rx)
}

#[tokio::test]
async fn outbound_webhook_fires_on_version_created() {
    let schema = "test_outbound_webhook_version";
    let (webhook_url, mut rx) = start_webhook_receiver().await;

    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![riley_leaderboards_core::config::WebhookConfig {
            url: webhook_url,
            events: vec![riley_leaderboards_core::config::WebhookEvent::VersionCreated],
            boards: vec![],
            secret: None,
        }],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    // Create board
    let resp = app.clone().oneshot(
        Request::post("/boards")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"slug":"wh-test","name":"Webhook Test","board_type":"ordered","sort_direction":"asc"}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Create entry
    let resp = app.clone().oneshot(
        Request::post("/boards/wh-test/entries")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"slug":"e1","name":"Entry 1"}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Create version — should fire webhook
    let resp = app.clone().oneshot(
        Request::post("/boards/wh-test/versions")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"placements":[{"entry_slug":"e1"}],"note":"test note"}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Wait for webhook delivery
    let (body, headers) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        rx.recv(),
    ).await.expect("webhook timeout").expect("no webhook received");

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["event"], "version.created");
    assert_eq!(json["board"]["slug"], "wh-test");
    assert_eq!(json["board"]["name"], "Webhook Test");
    assert_eq!(json["version"]["version_number"], 1);
    assert_eq!(json["version"]["note"], "test note");
    assert_eq!(headers.get("content-type").unwrap(), "application/json");

    cleanup_schema(&pool, schema).await;
}

/// Cleanup helper for webhook tests — drop schema and close pool.
async fn cleanup_schema(pool: &sqlx::PgPool, schema: &str) {
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema))
        .execute(pool)
        .await
        .expect("test cleanup: failed to drop schema");
    pool.close().await;
}

#[tokio::test]
async fn outbound_webhook_hmac_signature() {
    let schema = "test_outbound_webhook_hmac";
    let (webhook_url, mut rx) = start_webhook_receiver().await;

    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![riley_leaderboards_core::config::WebhookConfig {
            url: webhook_url,
            events: vec![riley_leaderboards_core::config::WebhookEvent::BoardCreated],
            boards: vec![],
            secret: Some(ConfigValue::new("test-webhook-secret")),
        }],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    // Create board — should fire webhook with HMAC signature
    let resp = app.clone().oneshot(
        Request::post("/boards")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"slug":"hmac-test","name":"HMAC Test","board_type":"ordered","sort_direction":"asc"}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Wait for webhook delivery
    let (body, headers) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        rx.recv(),
    ).await.expect("webhook timeout").expect("no webhook received");

    // Verify signature header exists and is correct
    let sig_header = headers.get("x-webhook-signature-256").expect("missing signature header");
    let hex_sig = sig_header.strip_prefix("sha256=").expect("signature should start with sha256=");

    // Verify HMAC
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(b"test-webhook-secret").unwrap();
    mac.update(body.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    assert_eq!(hex_sig, expected, "HMAC signature mismatch");

    cleanup_schema(&pool, schema).await;
}

#[tokio::test]
async fn outbound_webhook_board_filter() {
    let schema = "test_outbound_webhook_filter";
    let (webhook_url, mut rx) = start_webhook_receiver().await;

    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![riley_leaderboards_core::config::WebhookConfig {
            url: webhook_url,
            events: vec![riley_leaderboards_core::config::WebhookEvent::BoardCreated],
            boards: vec!["dc-*".to_string()],
            secret: None,
        }],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    // Create board that DOES NOT match filter — should NOT fire webhook
    let resp = app.clone().oneshot(
        Request::post("/boards")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"slug":"nfl-rankings","name":"NFL Rankings","board_type":"ordered","sort_direction":"asc"}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Create board that DOES match filter — should fire webhook
    let resp = app.clone().oneshot(
        Request::post("/boards")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"slug":"dc-sandwiches","name":"DC Sandwiches","board_type":"ordered","sort_direction":"asc"}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Should receive exactly one webhook (for dc-sandwiches, not nfl-rankings)
    let (body, _) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        rx.recv(),
    ).await.expect("webhook timeout").expect("no webhook received");

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["board"]["slug"], "dc-sandwiches");

    // Verify no second webhook arrived (for nfl-rankings)
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        rx.recv(),
    ).await;
    assert!(result.is_err(), "should not have received a webhook for nfl-rankings");

    cleanup_schema(&pool, schema).await;
}

#[tokio::test]
async fn outbound_webhook_fires_on_board_delete() {
    let schema = "test_outbound_webhook_delete";
    let (webhook_url, mut rx) = start_webhook_receiver().await;

    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        auth: None,
        sync: None,
        webhooks: vec![riley_leaderboards_core::config::WebhookConfig {
            url: webhook_url,
            events: vec![riley_leaderboards_core::config::WebhookEvent::BoardDeleted],
            boards: vec![],
            secret: None,
        }],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool: pool.clone(),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
    });
    let app = build_router(state);

    // Create board (no webhook for create since we're only listening for delete)
    let resp = app.clone().oneshot(
        Request::post("/boards")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"slug":"del-test","name":"Delete Test","board_type":"ordered","sort_direction":"asc"}"#))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Delete board — should fire webhook
    let resp = app.clone().oneshot(
        Request::delete("/boards/del-test")
            .body(Body::empty())
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Wait for webhook delivery
    let (body, _) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        rx.recv(),
    ).await.expect("webhook timeout").expect("no webhook received");

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["event"], "board.deleted");
    assert_eq!(json["board"]["slug"], "del-test");
    assert_eq!(json["board"]["name"], "Delete Test");
    assert!(json.get("version").is_none());

    cleanup_schema(&pool, schema).await;
}

// ── Collection CRUD ──────────────────────────────────────────────────────

#[tokio::test]
async fn collection_create_list_get_update_delete() {
    let schema = "test_collection_crud";
    let (state, app) = setup(schema).await;

    // Create collection
    let resp = app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "food-rankings",
        "name": "Riley's Food Rankings",
        "metadata": { "description": "All my DC food lists" }
    })))).await.unwrap();
    assert_eq!(resp.status(), 201);
    let body = json_body(resp).await;
    assert_eq!(body["slug"], "food-rankings");
    assert_eq!(body["name"], "Riley's Food Rankings");
    assert_eq!(body["metadata"]["description"], "All my DC food lists");

    // List collections
    let resp = app.clone().oneshot(json_request("GET", "/collections", None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);

    // Get collection (empty boards)
    let resp = app.clone().oneshot(json_request("GET", "/collections/food-rankings", None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["slug"], "food-rankings");
    assert_eq!(body["boards"].as_array().unwrap().len(), 0);

    // Update collection
    let resp = app.clone().oneshot(json_request("PATCH", "/collections/food-rankings", Some(serde_json::json!({
        "name": "DC Food Rankings"
    })))).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["name"], "DC Food Rankings");
    assert_eq!(body["metadata"]["description"], "All my DC food lists");

    // Delete collection
    let resp = app.clone().oneshot(json_request("DELETE", "/collections/food-rankings", None)).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Verify deleted
    let resp = app.clone().oneshot(json_request("GET", "/collections/food-rankings", None)).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn collection_duplicate_slug_returns_409() {
    let schema = "test_collection_dup";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "dupes",
        "name": "Test"
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "dupes",
        "name": "Test 2"
    })))).await.unwrap();
    assert_eq!(resp.status(), 409);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn collection_invalid_slug_returns_400() {
    let schema = "test_collection_slug";
    let (state, app) = setup(schema).await;

    let resp = app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "INVALID SLUG",
        "name": "Test"
    })))).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn collection_empty_name_returns_400() {
    let schema = "test_collection_name";
    let (state, app) = setup(schema).await;

    let resp = app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "test",
        "name": ""
    })))).await.unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn collection_patch_can_clear_metadata_to_null() {
    let schema = "test_collection_meta_null";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "meta-test",
        "name": "Meta Test",
        "metadata": { "key": "value" }
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("PATCH", "/collections/meta-test", Some(serde_json::json!({
        "metadata": null
    })))).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert!(body["metadata"].is_null());

    cleanup(&state, schema).await;
}

// ── Board Membership ─────────────────────────────────────────────────────

#[tokio::test]
async fn collection_add_remove_boards() {
    let schema = "test_collection_boards";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "food",
        "name": "Food Rankings"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards", Some(serde_json::json!({
        "slug": "sandwiches",
        "name": "Best Sandwiches",
        "board_type": "ordered"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards", Some(serde_json::json!({
        "slug": "pizza",
        "name": "Best Pizza",
        "board_type": "ordered"
    })))).await.unwrap();

    // Add boards
    let resp = app.clone().oneshot(json_request("POST", "/collections/food/boards", Some(serde_json::json!({
        "board_slug": "sandwiches",
        "display_order": 1
    })))).await.unwrap();
    assert_eq!(resp.status(), 201);

    let resp = app.clone().oneshot(json_request("POST", "/collections/food/boards", Some(serde_json::json!({
        "board_slug": "pizza",
        "display_order": 2
    })))).await.unwrap();
    assert_eq!(resp.status(), 201);

    // Get collection with boards (ordered by display_order)
    let resp = app.clone().oneshot(json_request("GET", "/collections/food", None)).await.unwrap();
    let body = json_body(resp).await;
    let boards = body["boards"].as_array().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[0]["slug"], "sandwiches");
    assert_eq!(boards[0]["display_order"], 1);
    assert_eq!(boards[1]["slug"], "pizza");
    assert_eq!(boards[1]["display_order"], 2);

    // Remove a board
    let resp = app.clone().oneshot(json_request("DELETE", "/collections/food/boards/sandwiches", None)).await.unwrap();
    assert_eq!(resp.status(), 204);

    let resp = app.clone().oneshot(json_request("GET", "/collections/food", None)).await.unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["boards"].as_array().unwrap().len(), 1);
    assert_eq!(body["boards"][0]["slug"], "pizza");

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn collection_add_duplicate_board_returns_409() {
    let schema = "test_collection_dup_board";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "col",
        "name": "Collection"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards", Some(serde_json::json!({
        "slug": "board-a",
        "name": "Board A",
        "board_type": "ordered"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/collections/col/boards", Some(serde_json::json!({
        "board_slug": "board-a"
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("POST", "/collections/col/boards", Some(serde_json::json!({
        "board_slug": "board-a"
    })))).await.unwrap();
    assert_eq!(resp.status(), 409);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn collection_add_nonexistent_board_returns_404() {
    let schema = "test_collection_no_board";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "col",
        "name": "Collection"
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("POST", "/collections/col/boards", Some(serde_json::json!({
        "board_slug": "nonexistent"
    })))).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn collection_remove_nonexistent_membership_returns_404() {
    let schema = "test_collection_no_member";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "col",
        "name": "Collection"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards", Some(serde_json::json!({
        "slug": "board-a",
        "name": "Board A",
        "board_type": "ordered"
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("DELETE", "/collections/col/boards/board-a", None)).await.unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

// ── Board in multiple collections ────────────────────────────────────────

#[tokio::test]
async fn board_in_multiple_collections() {
    let schema = "test_multi_collection";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "food",
        "name": "Food"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "dc",
        "name": "DC Things"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards", Some(serde_json::json!({
        "slug": "sandwiches",
        "name": "Best Sandwiches",
        "board_type": "ordered"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/collections/food/boards", Some(serde_json::json!({
        "board_slug": "sandwiches"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/collections/dc/boards", Some(serde_json::json!({
        "board_slug": "sandwiches"
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("GET", "/collections/food", None)).await.unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["boards"].as_array().unwrap().len(), 1);

    let resp = app.clone().oneshot(json_request("GET", "/collections/dc", None)).await.unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["boards"].as_array().unwrap().len(), 1);

    cleanup(&state, schema).await;
}

// ── Cascading deletion ───────────────────────────────────────────────────

#[tokio::test]
async fn collection_delete_does_not_delete_boards() {
    let schema = "test_collection_cascade";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "temp",
        "name": "Temporary"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards", Some(serde_json::json!({
        "slug": "keeper",
        "name": "Should Survive",
        "board_type": "ordered"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/collections/temp/boards", Some(serde_json::json!({
        "board_slug": "keeper"
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("DELETE", "/collections/temp", None)).await.unwrap();
    assert_eq!(resp.status(), 204);

    let resp = app.clone().oneshot(json_request("GET", "/boards/keeper", None)).await.unwrap();
    assert_eq!(resp.status(), 200);

    cleanup(&state, schema).await;
}

#[tokio::test]
async fn board_delete_removes_from_collections() {
    let schema = "test_board_del_collection";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "col",
        "name": "Collection"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards", Some(serde_json::json!({
        "slug": "board-a",
        "name": "Board A",
        "board_type": "ordered"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/collections/col/boards", Some(serde_json::json!({
        "board_slug": "board-a"
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("DELETE", "/boards/board-a", None)).await.unwrap();
    assert_eq!(resp.status(), 204);

    let resp = app.clone().oneshot(json_request("GET", "/collections/col", None)).await.unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["boards"].as_array().unwrap().len(), 0);

    cleanup(&state, schema).await;
}

// ── Pagination ───────────────────────────────────────────────────────────

#[tokio::test]
async fn pagination_collections_list() {
    let schema = "test_collection_pagination";
    let (state, app) = setup(schema).await;

    for i in 0..3 {
        app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
            "slug": format!("col-{i}"),
            "name": format!("Collection {i}")
        })))).await.unwrap();
    }

    let resp = app.clone().oneshot(json_request("GET", "/collections?limit=2", None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert!(body["next_cursor"].is_string());

    let cursor = body["next_cursor"].as_str().unwrap();
    let url = format!("/collections?limit=2&cursor={}", urlencoding::encode(cursor));
    let resp = app.clone().oneshot(json_request("GET", &url, None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert!(body.get("next_cursor").is_none() || body["next_cursor"].is_null());

    cleanup(&state, schema).await;
}

// ── Collection board with latest_version ─────────────────────────────────

#[tokio::test]
async fn collection_board_shows_latest_version() {
    let schema = "test_collection_latest_ver";
    let (state, app) = setup(schema).await;

    app.clone().oneshot(json_request("POST", "/collections", Some(serde_json::json!({
        "slug": "food",
        "name": "Food"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards", Some(serde_json::json!({
        "slug": "sandwiches",
        "name": "Best Sandwiches",
        "board_type": "scored"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards/sandwiches/entries", Some(serde_json::json!({
        "slug": "bobs",
        "name": "Bob's Subs"
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/boards/sandwiches/versions", Some(serde_json::json!({
        "placements": [{"entry_slug": "bobs", "score": 9.5}]
    })))).await.unwrap();

    app.clone().oneshot(json_request("POST", "/collections/food/boards", Some(serde_json::json!({
        "board_slug": "sandwiches"
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("GET", "/collections/food", None)).await.unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["boards"][0]["latest_version"], 1);

    app.clone().oneshot(json_request("POST", "/boards/sandwiches/versions", Some(serde_json::json!({
        "placements": [{"entry_slug": "bobs", "score": 10.0}]
    })))).await.unwrap();

    let resp = app.clone().oneshot(json_request("GET", "/collections/food", None)).await.unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["boards"][0]["latest_version"], 2);

    cleanup(&state, schema).await;
}
