use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{
    Board, CreatePlacement, CreateVersion, DiffEntry, DiffMovedEntry, PlacementWithEntry, Version,
    VersionDiff, VersionWithPlacements,
};

pub async fn create(
    pool: &PgPool,
    board: &Board,
    input: &CreateVersion,
    max_versions: Option<i64>,
) -> Result<VersionWithPlacements> {
    if board.accumulative {
        return Err(Error::Validation(
            "cannot create versions directly on accumulative boards; use POST /boards/:slug/snapshot instead".to_string(),
        ));
    }

    // Pre-validate with the snapshot we have (fast-fail before opening a tx).
    validate_placements(board, &input.placements)?;

    let mut tx = pool.begin().await?;

    // Lock and re-fetch the board row. This serializes concurrent version
    // creation AND ensures we use the latest board state (sort_direction,
    // tier_config) in case a concurrent PATCH updated the board.
    let board = sqlx::query_as::<_, Board>("SELECT * FROM boards WHERE id = $1 FOR UPDATE")
        .bind(board.id)
        .fetch_one(&mut *tx)
        .await?;

    // Safety limit: max versions per board (checked inside tx after FOR UPDATE to avoid TOCTOU)
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

    // Re-validate with the locked board state in case tier_config or other
    // validation-relevant fields changed since the pre-check.
    validate_placements(&board, &input.placements)?;

    // Get next version number (now safe — board row is locked)
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
    .bind(&input.note)
    .bind(&input.metadata)
    .fetch_one(&mut *tx)
    .await?;

    // Batch-resolve all entry slugs to IDs in a single query
    let slugs: Vec<&str> = input.placements.iter().map(|p| p.entry_slug.as_str()).collect();
    let entries = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, slug, name FROM entries WHERE board_id = $1 AND slug = ANY($2)",
    )
    .bind(board.id)
    .bind(&slugs)
    .fetch_all(&mut *tx)
    .await?;

    let entry_map: HashMap<&str, (Uuid, &str, &str)> = entries
        .iter()
        .map(|(id, slug, name)| (slug.as_str(), (*id, slug.as_str(), name.as_str())))
        .collect();

    // Validate all slugs were found
    for p in &input.placements {
        if !entry_map.contains_key(p.entry_slug.as_str()) {
            return Err(Error::Validation(format!(
                "entry '{}' not found on board '{}'",
                p.entry_slug, board.slug
            )));
        }
    }

    // Create placements
    let mut placements = Vec::with_capacity(input.placements.len());
    for (i, p) in input.placements.iter().enumerate() {
        let (entry_id, entry_slug, entry_name) = entry_map[p.entry_slug.as_str()];

        // For ordered boards, use explicit position or derive from array order
        let position = match board.board_type.as_str() {
            "ordered" => Some(p.position.unwrap_or((i as i32) + 1)),
            "scored" => p.position, // Position is derived later if needed
            "tiered" => p.position, // Optional within-tier ordering
            _ => p.position,
        };

        let placement = sqlx::query_as::<_, PlacementWithEntry>(
            r#"INSERT INTO placements (version_id, entry_id, position, score, tier, metadata)
               VALUES ($1, $2, $3, $4, $5, $6)
               RETURNING
                   placements.id, placements.version_id, placements.entry_id,
                   placements.position, placements.score, placements.tier,
                   placements.metadata,
                   $7::text AS entry_slug, $8::text AS entry_name"#,
        )
        .bind(version.id)
        .bind(entry_id)
        .bind(position)
        .bind(p.score)
        .bind(&p.tier)
        .bind(&p.metadata)
        .bind(entry_slug)
        .bind(entry_name)
        .fetch_one(&mut *tx)
        .await?;

        placements.push(placement);
    }

    // For scored boards, derive positions from scores after all placements are inserted
    if board.board_type == "scored" {
        derive_scored_positions(&mut tx, version.id, &board.sort_direction).await?;
        // Re-fetch placements with derived positions
        placements = fetch_placements(&mut *tx, version.id).await?;
    }

    tx.commit().await?;

    Ok(VersionWithPlacements {
        version,
        placements,
    })
}

pub async fn list(pool: &PgPool, board_id: Uuid) -> Result<Vec<Version>> {
    let versions = sqlx::query_as::<_, Version>(
        "SELECT * FROM versions WHERE board_id = $1 ORDER BY version_number DESC",
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;
    Ok(versions)
}

pub async fn list_paginated(
    pool: &PgPool,
    board_id: Uuid,
    params: &crate::models::PaginationParams,
) -> Result<crate::models::PaginatedResponse<Version>> {
    let limit = params.effective_limit();
    let cursor = params.decode_cursor()?;

    let versions = if let Some((ts, id)) = cursor {
        sqlx::query_as::<_, Version>(
            r#"SELECT * FROM versions
               WHERE board_id = $1 AND (created_at, id) < ($2, $3)
               ORDER BY created_at DESC, id DESC
               LIMIT $4"#,
        )
        .bind(board_id)
        .bind(ts)
        .bind(id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Version>(
            "SELECT * FROM versions WHERE board_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
        )
        .bind(board_id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    };

    let has_more = versions.len() as i64 > limit;
    let items: Vec<Version> = versions.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items
            .last()
            .map(|v| crate::models::encode_cursor(&v.created_at, &v.id))
    } else {
        None
    };

    Ok(crate::models::PaginatedResponse { items, next_cursor })
}

pub async fn get_by_number(
    pool: &PgPool,
    board_id: Uuid,
    version_number: i32,
) -> Result<VersionWithPlacements> {
    let version = sqlx::query_as::<_, Version>(
        "SELECT * FROM versions WHERE board_id = $1 AND version_number = $2",
    )
    .bind(board_id)
    .bind(version_number)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| Error::NotFound(format!("version {version_number} not found")))?;

    let placements = fetch_placements(pool, version.id).await?;

    Ok(VersionWithPlacements {
        version,
        placements,
    })
}

pub async fn get_latest(pool: &PgPool, board_id: Uuid) -> Result<VersionWithPlacements> {
    let version = sqlx::query_as::<_, Version>(
        "SELECT * FROM versions WHERE board_id = $1 ORDER BY version_number DESC LIMIT 1",
    )
    .bind(board_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| Error::NotFound("no versions exist for this board".to_string()))?;

    let placements = fetch_placements(pool, version.id).await?;

    Ok(VersionWithPlacements {
        version,
        placements,
    })
}

async fn fetch_placements<'e, E>(executor: E, version_id: Uuid) -> Result<Vec<PlacementWithEntry>>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    // For tiered boards, use the board's tier_config to order by tier position.
    // For non-tiered boards, tier is null and the LATERAL join produces no match,
    // so tier_ord defaults to a high value and ordering falls through to position.
    let placements = sqlx::query_as::<_, PlacementWithEntry>(
        r#"SELECT p.id, p.version_id, p.entry_id, p.position, p.score, p.tier, p.metadata,
                  e.slug AS entry_slug, e.name AS entry_name
           FROM placements p
           JOIN entries e ON e.id = p.entry_id
           JOIN versions v ON v.id = p.version_id
           JOIN boards b ON b.id = v.board_id
           LEFT JOIN LATERAL (
               SELECT (t.obj->>'position')::int AS tier_ord
               FROM jsonb_array_elements(b.tier_config->'tiers') AS t(obj)
               WHERE t.obj->>'key' = p.tier
               LIMIT 1
           ) tc ON true
           WHERE p.version_id = $1
           ORDER BY COALESCE(tc.tier_ord, 2147483647) ASC,
                    COALESCE(p.position, 2147483647) ASC,
                    e.name ASC"#,
    )
    .bind(version_id)
    .fetch_all(executor)
    .await?;
    Ok(placements)
}

pub async fn derive_scored_positions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    version_id: Uuid,
    sort_direction: &str,
) -> Result<()> {
    let order = if sort_direction == "asc" {
        "ASC"
    } else {
        "DESC"
    };

    // Use a window function to assign positions based on score.
    // Tiebreaker on entry_id (stable across versions) rather than placements.id
    // (which changes per version, causing non-deterministic ordering for ties).
    let query = format!(
        r#"UPDATE placements
           SET position = ranked.pos
           FROM (
               SELECT id, ROW_NUMBER() OVER (ORDER BY score {order} NULLS LAST, entry_id ASC) AS pos
               FROM placements
               WHERE version_id = $1
           ) ranked
           WHERE placements.id = ranked.id"#
    );

    sqlx::query(&query)
        .bind(version_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

pub async fn since(
    pool: &PgPool,
    board_id: Uuid,
    after_version: i32,
    limit: i64,
) -> Result<Vec<Version>> {
    let versions = sqlx::query_as::<_, Version>(
        r#"SELECT * FROM versions
           WHERE board_id = $1 AND version_number > $2
           ORDER BY version_number ASC
           LIMIT $3"#,
    )
    .bind(board_id)
    .bind(after_version)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(versions)
}

pub async fn diff(
    pool: &PgPool,
    board_id: Uuid,
    from_number: i32,
    to_number: i32,
) -> Result<VersionDiff> {
    // Fetch both versions (validates they exist)
    let from = get_by_number(pool, board_id, from_number).await?;
    let to = get_by_number(pool, board_id, to_number).await?;

    // Build lookup maps keyed by entry slug
    let from_map: std::collections::HashMap<&str, &PlacementWithEntry> = from
        .placements
        .iter()
        .map(|p| (p.entry_slug.as_str(), p))
        .collect();
    let to_map: std::collections::HashMap<&str, &PlacementWithEntry> = to
        .placements
        .iter()
        .map(|p| (p.entry_slug.as_str(), p))
        .collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut moved = Vec::new();
    let mut unchanged = Vec::new();

    // Entries in `to` but not in `from` → added
    // Entries in both → check if moved or unchanged
    for p in &to.placements {
        let slug = p.entry_slug.as_str();
        match from_map.get(slug) {
            None => {
                added.push(DiffEntry {
                    entry_slug: p.entry_slug.clone(),
                    entry_name: p.entry_name.clone(),
                    position: p.position,
                    score: p.score,
                    tier: p.tier.clone(),
                });
            }
            Some(from_p) => {
                let pos_changed = from_p.position != p.position;
                let score_changed = !super::scores_equal(from_p.score, p.score);
                let tier_changed = from_p.tier != p.tier;

                if pos_changed || score_changed || tier_changed {
                    moved.push(DiffMovedEntry {
                        entry_slug: p.entry_slug.clone(),
                        entry_name: p.entry_name.clone(),
                        from_position: from_p.position,
                        to_position: p.position,
                        from_score: from_p.score,
                        to_score: p.score,
                        from_tier: from_p.tier.clone(),
                        to_tier: p.tier.clone(),
                    });
                } else {
                    unchanged.push(DiffEntry {
                        entry_slug: p.entry_slug.clone(),
                        entry_name: p.entry_name.clone(),
                        position: p.position,
                        score: p.score,
                        tier: p.tier.clone(),
                    });
                }
            }
        }
    }

    // Entries in `from` but not in `to` → removed
    for p in &from.placements {
        if !to_map.contains_key(p.entry_slug.as_str()) {
            removed.push(DiffEntry {
                entry_slug: p.entry_slug.clone(),
                entry_name: p.entry_name.clone(),
                position: p.position,
                score: p.score,
                tier: p.tier.clone(),
            });
        }
    }

    Ok(VersionDiff {
        from_version: from_number,
        to_version: to_number,
        added,
        removed,
        moved,
        unchanged,
    })
}

/// Public entry point for import validation (same rules as version creation).
pub fn validate_placements_for_import(
    board: &Board,
    placements: &[CreatePlacement],
) -> Result<()> {
    validate_placements(board, placements)
}

fn validate_placements(board: &Board, placements: &[CreatePlacement]) -> Result<()> {
    if placements.is_empty() {
        return Err(Error::Validation(
            "version must have at least one placement".to_string(),
        ));
    }

    // Check for duplicate entry slugs
    let mut seen_slugs = std::collections::HashSet::new();
    for p in placements {
        if !seen_slugs.insert(&p.entry_slug) {
            return Err(Error::Validation(format!(
                "duplicate entry slug '{}' in placements",
                p.entry_slug
            )));
        }
    }

    // Validate explicit positions across all board types: must be >= 1
    for p in placements {
        if let Some(pos) = p.position
            && pos < 1 {
                return Err(Error::Validation(format!(
                    "position must be >= 1, got {pos} for entry '{}'",
                    p.entry_slug
                )));
            }
    }

    match board.board_type.as_str() {
        "ordered" => {
            // Ordered boards: resolve all positions (explicit or implicit from
            // array order) then validate uniqueness across the full set.
            let mut seen_positions = std::collections::HashSet::new();
            for (i, p) in placements.iter().enumerate() {
                let resolved = p.position.unwrap_or((i as i32) + 1);
                if !seen_positions.insert(resolved) {
                    return Err(Error::Validation(format!(
                        "duplicate position {resolved} in ordered board placements"
                    )));
                }
            }
        }
        "scored" => {
            // Scored boards: score is required and must be finite
            for p in placements {
                match p.score {
                    None => {
                        return Err(Error::Validation(format!(
                            "score is required for scored board, missing for entry '{}'",
                            p.entry_slug
                        )));
                    }
                    Some(s) if s.is_nan() || s.is_infinite() => {
                        return Err(Error::Validation(format!(
                            "score must be a finite number for entry '{}'",
                            p.entry_slug
                        )));
                    }
                    _ => {}
                }
            }
        }
        "tiered" => {
            // Tiered boards: tier is required, validate against tier_config if present
            let valid_tiers: Option<Vec<String>> =
                board.tier_config.as_ref().and_then(|tc| {
                    tc.get("tiers").and_then(|tiers| {
                        tiers.as_array().map(|arr| {
                            arr.iter()
                                .filter_map(|t| t.get("key").and_then(|k| k.as_str()).map(String::from))
                                .collect()
                        })
                    })
                });

            for p in placements {
                let tier = p.tier.as_ref().ok_or_else(|| {
                    Error::Validation(format!(
                        "tier is required for tiered board, missing for entry '{}'",
                        p.entry_slug
                    ))
                })?;

                if let Some(ref valid) = valid_tiers
                    && !valid.contains(tier) {
                        return Err(Error::Validation(format!(
                            "invalid tier '{}' for entry '{}': valid tiers are {:?}",
                            tier, p.entry_slug, valid
                        )));
                    }
            }
        }
        _ => {}
    }

    Ok(())
}

