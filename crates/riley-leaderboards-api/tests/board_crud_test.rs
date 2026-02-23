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
    assert_eq!(refs.as_array().unwrap().len(), 2);

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
    assert_eq!(refs.as_array().unwrap().len(), 1);

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
    assert_eq!(entries.as_array().unwrap().len(), 3);

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
    assert_eq!(versions.as_array().unwrap().len(), 2);

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
