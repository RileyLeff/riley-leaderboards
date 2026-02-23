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
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState { pool, config });
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
    assert_eq!(boards.as_array().unwrap().len(), 1);

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
    assert_eq!(entries.as_array().unwrap().len(), 1);

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
    assert_eq!(versions.as_array().unwrap().len(), 2);
    // Newest first
    assert_eq!(versions[0]["version_number"], 2);
    assert_eq!(versions[1]["version_number"], 1);

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

    app.clone().oneshot(json_request(
        "POST",
        "/boards",
        Some(serde_json::json!({
            "slug": "tiers",
            "name": "Tiers",
            "board_type": "tiered"
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
