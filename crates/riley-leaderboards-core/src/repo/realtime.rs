//! Redis-backed operations for realtime boards.
//!
//! Key layout (with configurable prefix, default `rl`):
//! - `{prefix}:board:{slug}:scores`  — sorted set (member = entry_slug, score = score)
//! - `{prefix}:board:{slug}:entries` — hash (field = entry_slug, value = entry_name)

use redis::AsyncCommands;
use sqlx::PgPool;
use tracing;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{Board, PlacementWithEntry, SubmitScore, Version, VersionWithPlacements};

fn scores_key(prefix: &str, slug: &str) -> String {
    format!("{prefix}:board:{slug}:scores")
}

fn entries_key(prefix: &str, slug: &str) -> String {
    format!("{prefix}:board:{slug}:entries")
}

/// Submit a score to a realtime board.
///
/// 1. Upserts the entry in Postgres (so entry_id exists for snapshots).
/// 2. Writes score to Redis sorted set.
/// 3. Stores entry name in Redis hash.
pub async fn submit(
    pool: &PgPool,
    redis: &mut redis::aio::ConnectionManager,
    board: &Board,
    input: &SubmitScore,
    prefix: &str,
) -> Result<()> {
    if !board.realtime {
        return Err(Error::Validation(
            "realtime submit called on non-realtime board".to_string(),
        ));
    }
    if input.score.is_nan() || input.score.is_infinite() {
        return Err(Error::Validation(
            "score must be a finite number".to_string(),
        ));
    }
    super::validate_slug(&input.entry_slug)?;
    super::validate_name(&input.entry_name)?;

    // Upsert entry in Postgres so it exists for snapshots
    sqlx::query(
        r#"INSERT INTO entries (board_id, slug, name)
           VALUES ($1, $2, $3)
           ON CONFLICT (board_id, slug) DO UPDATE SET name = $3, updated_at = now()"#,
    )
    .bind(board.id)
    .bind(&input.entry_slug)
    .bind(&input.entry_name)
    .execute(pool)
    .await
    .map_err(Error::Database)?;

    // Write to Redis
    let sk = scores_key(prefix, &board.slug);
    let ek = entries_key(prefix, &board.slug);

    redis::pipe()
        .atomic()
        .zadd(&sk, &input.entry_slug, input.score)
        .hset(&ek, &input.entry_slug, &input.entry_name)
        .query_async::<()>(redis)
        .await
        .map_err(|e| Error::ServiceUnavailable(format!("Redis error: {e}")))?;

    Ok(())
}

/// Read current standings from Redis for a realtime board.
///
/// Returns a `VersionWithPlacements` with a synthetic version (version_number = 0,
/// id = nil UUID) representing the live state.
pub async fn latest(
    redis: &mut redis::aio::ConnectionManager,
    board: &Board,
    prefix: &str,
) -> Result<VersionWithPlacements> {
    let sk = scores_key(prefix, &board.slug);
    let ek = entries_key(prefix, &board.slug);

    // Read all entries with scores from the sorted set
    let entries_with_scores: Vec<(String, f64)> = if board.sort_direction == "asc" {
        redis
            .zrange_withscores(&sk, 0, -1)
            .await
            .map_err(|e| Error::ServiceUnavailable(format!("Redis error: {e}")))?
    } else {
        redis
            .zrevrange_withscores(&sk, 0, -1)
            .await
            .map_err(|e| Error::ServiceUnavailable(format!("Redis error: {e}")))?
    };

    // Batch-fetch entry names from the hash
    let entry_slugs: Vec<&str> = entries_with_scores.iter().map(|(s, _)| s.as_str()).collect();
    let entry_names: Vec<Option<String>> = if entry_slugs.is_empty() {
        vec![]
    } else {
        redis
            .hget(&ek, &entry_slugs)
            .await
            .map_err(|e| Error::ServiceUnavailable(format!("Redis error: {e}")))?
    };

    let placements: Vec<PlacementWithEntry> = entries_with_scores
        .iter()
        .zip(entry_names.iter())
        .enumerate()
        .map(|(i, ((slug, score), name))| PlacementWithEntry {
            id: Uuid::nil(),
            version_id: Uuid::nil(),
            entry_id: Uuid::nil(),
            position: Some((i + 1) as i32),
            score: Some(*score),
            tier: None,
            metadata: None,
            entry_slug: slug.clone(),
            entry_name: name.clone().unwrap_or_else(|| slug.clone()),
        })
        .collect();

    Ok(VersionWithPlacements {
        version: Version {
            id: Uuid::nil(),
            board_id: board.id,
            version_number: 0, // 0 indicates live state
            note: None,
            metadata: None,
            created_at: chrono::Utc::now(),
        },
        placements,
    })
}

/// Snapshot Redis state into a Postgres version.
///
/// Acquires the Postgres FOR UPDATE lock first, then reads Redis to minimize
/// the TOCTOU window for concurrent score submissions. Optionally clears
/// the Redis state if `board.clear_on_snapshot` is true.
pub async fn snapshot(
    pool: &PgPool,
    redis: &mut redis::aio::ConnectionManager,
    board: &Board,
    note: Option<&str>,
    metadata: Option<&serde_json::Value>,
    prefix: &str,
    max_versions: Option<i64>,
    max_entries: Option<usize>,
) -> Result<VersionWithPlacements> {
    let sk = scores_key(prefix, &board.slug);
    let ek = entries_key(prefix, &board.slug);

    let mut tx = pool.begin().await.map_err(Error::Database)?;

    // Lock the board row to serialize concurrent snapshots.
    // Acquired BEFORE reading Redis to minimize the TOCTOU window.
    let board = sqlx::query_as::<_, Board>("SELECT * FROM boards WHERE id = $1 FOR UPDATE")
        .bind(board.id)
        .fetch_one(&mut *tx)
        .await?;

    // Safety limit: max versions per board (checked inside tx after FOR UPDATE)
    if let Some(max) = max_versions {
        let version_count: (i64,) =
            sqlx::query_as("SELECT count(*) FROM versions WHERE board_id = $1")
                .bind(board.id)
                .fetch_one(&mut *tx)
                .await
                .map_err(Error::Database)?;
        if version_count.0 >= max {
            return Err(Error::Validation(format!(
                "board has too many versions ({}, max {max})",
                version_count.0,
            )));
        }
    }

    // Read all entries with scores from Redis (fetch order doesn't matter
    // since derive_scored_positions re-derives positions from sort_direction).
    let entries_with_scores: Vec<(String, f64)> = redis
        .zrevrange_withscores(&sk, 0, -1)
        .await
        .map_err(|e| Error::ServiceUnavailable(format!("Redis error: {e}")))?;

    if entries_with_scores.is_empty() {
        return Err(Error::Validation(
            "no scores in Redis to snapshot".to_string(),
        ));
    }

    // Safety limit: max entries per version
    if let Some(max) = max_entries {
        if entries_with_scores.len() > max {
            return Err(Error::Validation(format!(
                "too many entries to snapshot ({}, max {max})",
                entries_with_scores.len(),
            )));
        }
    }

    // Get next version number
    let next_number: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM versions WHERE board_id = $1",
    )
    .bind(board.id)
    .fetch_one(&mut *tx)
    .await?;

    // Create version
    let version = sqlx::query_as::<_, Version>(
        r#"INSERT INTO versions (board_id, version_number, note, metadata)
           VALUES ($1, $2, $3, $4)
           RETURNING *"#,
    )
    .bind(board.id)
    .bind(next_number)
    .bind(note)
    .bind(metadata)
    .fetch_one(&mut *tx)
    .await?;

    // Batch-fetch entry IDs from Postgres to avoid O(N) individual lookups
    let entry_slugs: Vec<&str> = entries_with_scores.iter().map(|(s, _)| s.as_str()).collect();
    let entry_rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, slug FROM entries WHERE board_id = $1 AND slug = ANY($2)",
    )
    .bind(board.id)
    .bind(&entry_slugs)
    .fetch_all(&mut *tx)
    .await?;

    let entry_map: std::collections::HashMap<String, Uuid> =
        entry_rows.into_iter().map(|(id, slug)| (slug, id)).collect();

    // Insert placements
    for (entry_slug, score) in &entries_with_scores {
        let entry_id = entry_map.get(entry_slug).ok_or_else(|| {
            Error::Internal(format!(
                "entry '{entry_slug}' exists in Redis but not in Postgres"
            ))
        })?;

        sqlx::query(
            r#"INSERT INTO placements (version_id, entry_id, score)
               VALUES ($1, $2, $3)"#,
        )
        .bind(version.id)
        .bind(entry_id)
        .bind(score)
        .execute(&mut *tx)
        .await?;
    }

    // Derive positions from scores
    super::versions::derive_scored_positions(&mut tx, version.id, &board.sort_direction).await?;

    // Fetch final placements with entry info
    let placements = sqlx::query_as::<_, PlacementWithEntry>(
        r#"SELECT p.id, p.version_id, p.entry_id, p.position, p.score, p.tier, p.metadata,
                  e.slug AS entry_slug, e.name AS entry_name
           FROM placements p
           JOIN entries e ON e.id = p.entry_id
           WHERE p.version_id = $1
           ORDER BY COALESCE(p.position, 2147483647) ASC, e.name ASC"#,
    )
    .bind(version.id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await.map_err(Error::Database)?;

    // Clear Redis state if configured (best-effort: Postgres commit is authoritative)
    if board.clear_on_snapshot {
        if let Err(e) = redis::pipe()
            .atomic()
            .del(&sk)
            .del(&ek)
            .query_async::<()>(redis)
            .await
        {
            tracing::error!("failed to clear Redis after snapshot for board '{}': {e}", board.slug);
        }
    }

    Ok(VersionWithPlacements {
        version,
        placements,
    })
}

/// Clear Redis state for a board (used when deleting a realtime board).
pub async fn clear(
    redis: &mut redis::aio::ConnectionManager,
    board_slug: &str,
    prefix: &str,
) -> Result<()> {
    let sk = scores_key(prefix, board_slug);
    let ek = entries_key(prefix, board_slug);

    let _: () = redis::pipe()
        .atomic()
        .del(&sk)
        .del(&ek)
        .query_async(redis)
        .await
        .map_err(|e| Error::ServiceUnavailable(format!("Redis error: {e}")))?;

    Ok(())
}
