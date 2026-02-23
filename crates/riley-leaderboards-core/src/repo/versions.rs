use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{
    Board, CreatePlacement, CreateVersion, PlacementWithEntry, Version, VersionWithPlacements,
};

pub async fn create(
    pool: &PgPool,
    board: &Board,
    input: &CreateVersion,
) -> Result<VersionWithPlacements> {
    validate_placements(board, &input.placements)?;

    let mut tx = pool.begin().await?;

    // Lock the board row to serialize concurrent version creation.
    // Without this, two transactions could read the same MAX(version_number)
    // and both try to insert the same next number.
    sqlx::query("SELECT id FROM boards WHERE id = $1 FOR UPDATE")
        .bind(board.id)
        .fetch_one(&mut *tx)
        .await?;

    // Get next version number (now safe — board row is locked)
    let next_number: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM versions WHERE board_id = $1",
    )
    .bind(board.id)
    .fetch_one(&mut *tx)
    .await?;

    // Create version
    let version = sqlx::query_as::<_, Version>(
        r#"INSERT INTO versions (board_id, version_number, note)
           VALUES ($1, $2, $3)
           RETURNING *"#,
    )
    .bind(board.id)
    .bind(next_number)
    .bind(&input.note)
    .fetch_one(&mut *tx)
    .await?;

    // Resolve entry slugs to IDs and create placements
    let mut placements = Vec::with_capacity(input.placements.len());
    for (i, p) in input.placements.iter().enumerate() {
        let entry = sqlx::query_as::<_, (Uuid, String, String)>(
            "SELECT id, slug, name FROM entries WHERE board_id = $1 AND slug = $2",
        )
        .bind(board.id)
        .bind(&p.entry_slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            Error::Validation(format!(
                "entry '{}' not found on board '{}'",
                p.entry_slug, board.slug
            ))
        })?;

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
        .bind(entry.0)
        .bind(position)
        .bind(p.score)
        .bind(&p.tier)
        .bind(&p.metadata)
        .bind(&entry.1)
        .bind(&entry.2)
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

async fn derive_scored_positions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    version_id: Uuid,
    sort_direction: &str,
) -> Result<()> {
    let order = if sort_direction == "asc" {
        "ASC"
    } else {
        "DESC"
    };

    // Use a window function to assign positions based on score
    let query = format!(
        r#"UPDATE placements
           SET position = ranked.pos
           FROM (
               SELECT id, ROW_NUMBER() OVER (ORDER BY score {order} NULLS LAST) AS pos
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

    match board.board_type.as_str() {
        "ordered" => {
            // Ordered boards: positions are optional (derived from array order)
            // but if provided, must be positive and unique.
            let mut seen_positions = std::collections::HashSet::new();
            for p in placements {
                if let Some(pos) = p.position {
                    if pos < 1 {
                        return Err(Error::Validation(format!(
                            "position must be >= 1, got {pos} for entry '{}'",
                            p.entry_slug
                        )));
                    }
                    if !seen_positions.insert(pos) {
                        return Err(Error::Validation(format!(
                            "duplicate position {pos} in ordered board placements"
                        )));
                    }
                }
            }
        }
        "scored" => {
            // Scored boards: score is required
            for p in placements {
                if p.score.is_none() {
                    return Err(Error::Validation(format!(
                        "score is required for scored board, missing for entry '{}'",
                        p.entry_slug
                    )));
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

                if let Some(ref valid) = valid_tiers {
                    if !valid.contains(tier) {
                        return Err(Error::Validation(format!(
                            "invalid tier '{}' for entry '{}': valid tiers are {:?}",
                            tier, p.entry_slug, valid
                        )));
                    }
                }
            }
        }
        _ => {}
    }

    Ok(())
}
