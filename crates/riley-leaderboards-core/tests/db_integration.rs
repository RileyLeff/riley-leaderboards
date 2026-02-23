//! Integration tests for database connection and migration.
//!
//! Requires a running Postgres 18 instance. Run `docker compose up -d` from
//! the repo root to start one. Set TEST_DATABASE_URL to override.
//! Default: postgresql://riley_leaderboards:riley_leaderboards_test@localhost:15433/riley_leaderboards_test

use riley_leaderboards_core::config::{ConfigValue, DatabaseConfig};
use riley_leaderboards_core::db;

fn test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://riley_leaderboards:riley_leaderboards_test@localhost:15433/riley_leaderboards_test".to_string()
    })
}

fn make_config(schema: Option<&str>) -> DatabaseConfig {
    DatabaseConfig {
        url: ConfigValue::Literal(test_db_url()),
        max_connections: 2,
        schema: schema.map(String::from),
    }
}

#[tokio::test]
async fn connect_default_schema() {
    let config = make_config(None);
    let pool = db::connect(&config).await.expect("connect failed");

    let row: (i32,) = sqlx::query_as("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("query failed");
    assert_eq!(row.0, 1);
    pool.close().await;
}

#[tokio::test]
async fn connect_custom_schema() {
    let schema_name = "test_custom_schema_connect";
    let config = make_config(Some(schema_name));
    let pool = db::connect(&config).await.expect("connect failed");

    // Verify the schema was created
    let exists: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)")
            .bind(schema_name)
            .fetch_one(&pool)
            .await
            .expect("schema check failed");
    assert!(exists.0, "custom schema should exist");

    // Verify search_path includes our schema
    let search_path: (String,) = sqlx::query_as("SHOW search_path")
        .fetch_one(&pool)
        .await
        .expect("show search_path failed");
    assert!(
        search_path.0.contains(schema_name),
        "search_path should contain custom schema, got: {}",
        search_path.0
    );

    // Clean up
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema_name))
        .execute(&pool)
        .await
        .ok();
    pool.close().await;
}

#[tokio::test]
async fn migrate_default_schema() {
    // Use a dedicated schema so we don't pollute the public schema
    let schema_name = "test_migrate_default";
    let config = make_config(Some(schema_name));
    let pool = db::connect(&config).await.expect("connect failed");

    db::migrate(&pool).await.expect("migration failed");

    // Verify tables were created in the custom schema
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = $1 ORDER BY table_name",
    )
    .bind(schema_name)
    .fetch_all(&pool)
    .await
    .expect("table list failed");

    let table_names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();
    assert!(table_names.contains(&"boards"), "boards table should exist");
    assert!(table_names.contains(&"entries"), "entries table should exist");
    assert!(table_names.contains(&"versions"), "versions table should exist");
    assert!(table_names.contains(&"placements"), "placements table should exist");
    assert!(
        table_names.contains(&"board_references"),
        "board_references table should exist"
    );
    assert!(
        table_names.contains(&"accumulated_scores"),
        "accumulated_scores table should exist"
    );
    assert!(
        table_names.contains(&"_sqlx_migrations"),
        "_sqlx_migrations should be in custom schema"
    );

    // Clean up
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema_name))
        .execute(&pool)
        .await
        .ok();
    pool.close().await;
}

#[tokio::test]
async fn two_schemas_coexist() {
    let schema_a = "test_coexist_a";
    let schema_b = "test_coexist_b";

    let config_a = make_config(Some(schema_a));
    let config_b = make_config(Some(schema_b));

    let pool_a = db::connect(&config_a).await.expect("connect a failed");
    let pool_b = db::connect(&config_b).await.expect("connect b failed");

    db::migrate(&pool_a).await.expect("migrate a failed");
    db::migrate(&pool_b).await.expect("migrate b failed");

    // Both schemas should have their own boards table
    let count_a: (i64,) = sqlx::query_as("SELECT count(*) FROM boards")
        .fetch_one(&pool_a)
        .await
        .expect("count a failed");
    let count_b: (i64,) = sqlx::query_as("SELECT count(*) FROM boards")
        .fetch_one(&pool_b)
        .await
        .expect("count b failed");

    assert_eq!(count_a.0, 0);
    assert_eq!(count_b.0, 0);

    // Insert into schema A's boards, verify it doesn't appear in B
    sqlx::query(
        "INSERT INTO boards (slug, name, board_type) VALUES ('test-board', 'Test', 'ordered')",
    )
    .execute(&pool_a)
    .await
    .expect("insert a failed");

    let count_a: (i64,) = sqlx::query_as("SELECT count(*) FROM boards")
        .fetch_one(&pool_a)
        .await
        .expect("recount a failed");
    let count_b: (i64,) = sqlx::query_as("SELECT count(*) FROM boards")
        .fetch_one(&pool_b)
        .await
        .expect("recount b failed");

    assert_eq!(count_a.0, 1, "schema A should have 1 board");
    assert_eq!(count_b.0, 0, "schema B should still have 0 boards");

    // Clean up
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema_a))
        .execute(&pool_a)
        .await
        .ok();
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema_b))
        .execute(&pool_b)
        .await
        .ok();
    pool_a.close().await;
    pool_b.close().await;
}
