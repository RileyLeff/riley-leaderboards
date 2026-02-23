use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{Board, BoardSummary, CreateBoard, Nullable, UpdateBoard};

pub async fn create(pool: &PgPool, input: &CreateBoard) -> Result<Board> {
    super::validate_slug(&input.slug)?;
    super::validate_name(&input.name)?;
    validate_board_type(&input.board_type)?;
    validate_sort_direction(&input.sort_direction)?;
    if input.board_type == "tiered" {
        validate_tier_config(input.tier_config.as_ref())?;
    }
    if input.accumulative && input.board_type != "scored" {
        return Err(Error::Validation(
            "accumulative boards must have board_type 'scored'".to_string(),
        ));
    }
    if input.realtime && !(input.accumulative && input.board_type == "scored") {
        return Err(Error::Validation(
            "realtime boards must be accumulative and scored".to_string(),
        ));
    }
    if input.clear_on_snapshot && !input.realtime {
        return Err(Error::Validation(
            "clear_on_snapshot requires realtime to be true".to_string(),
        ));
    }

    let board = sqlx::query_as::<_, Board>(
        r#"INSERT INTO boards (slug, name, board_type, sort_direction, tier_config, metadata, accumulative, realtime, clear_on_snapshot)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
           RETURNING *"#,
    )
    .bind(&input.slug)
    .bind(&input.name)
    .bind(&input.board_type)
    .bind(&input.sort_direction)
    .bind(&input.tier_config)
    .bind(&input.metadata)
    .bind(input.accumulative)
    .bind(input.realtime)
    .bind(input.clear_on_snapshot)
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            Error::Conflict(format!("board with slug '{}' already exists", input.slug))
        }
        _ => Error::Database(e),
    })?;

    Ok(board)
}

pub async fn list(pool: &PgPool) -> Result<Vec<Board>> {
    let boards = sqlx::query_as::<_, Board>("SELECT * FROM boards ORDER BY created_at DESC")
        .fetch_all(pool)
        .await?;
    Ok(boards)
}

pub async fn list_paginated(
    pool: &PgPool,
    params: &crate::models::PaginationParams,
) -> Result<crate::models::PaginatedResponse<Board>> {
    let limit = params.effective_limit();
    let cursor = params.decode_cursor()?;

    let boards = if let Some((ts, id)) = cursor {
        sqlx::query_as::<_, Board>(
            r#"SELECT * FROM boards
               WHERE (created_at, id) < ($1, $2)
               ORDER BY created_at DESC, id DESC
               LIMIT $3"#,
        )
        .bind(ts)
        .bind(id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Board>(
            "SELECT * FROM boards ORDER BY created_at DESC, id DESC LIMIT $1",
        )
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    };

    let has_more = boards.len() as i64 > limit;
    let items: Vec<Board> = boards.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items
            .last()
            .map(|b| crate::models::encode_cursor(&b.created_at, &b.id))
    } else {
        None
    };

    Ok(crate::models::PaginatedResponse { items, next_cursor })
}

pub async fn get_by_slug(pool: &PgPool, slug: &str) -> Result<Board> {
    sqlx::query_as::<_, Board>("SELECT * FROM boards WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| Error::NotFound(format!("board '{slug}' not found")))
}

pub async fn get_summary(pool: &PgPool, slug: &str) -> Result<BoardSummary> {
    let board = get_by_slug(pool, slug).await?;

    let latest_version: Option<(i32,)> = sqlx::query_as(
        "SELECT version_number FROM versions WHERE board_id = $1 ORDER BY version_number DESC LIMIT 1",
    )
    .bind(board.id)
    .fetch_optional(pool)
    .await?;

    let entry_count: (i64,) = sqlx::query_as("SELECT count(*) FROM entries WHERE board_id = $1")
        .bind(board.id)
        .fetch_one(pool)
        .await?;

    Ok(BoardSummary {
        board,
        latest_version: latest_version.map(|v| v.0),
        entry_count: entry_count.0,
    })
}

pub async fn update(pool: &PgPool, slug: &str, input: &UpdateBoard) -> Result<Board> {
    if let Some(ref name) = input.name {
        super::validate_name(name)?;
    }
    if let Some(ref sd) = input.sort_direction {
        validate_sort_direction(sd)?;
    }

    let board = get_by_slug(pool, slug).await?;

    // Validate tier_config on tiered boards: reject null (tiered boards require tier_config)
    if board.board_type == "tiered" {
        match &input.tier_config {
            Nullable::Value(v) => validate_tier_config(Some(v))?,
            Nullable::Null => {
                return Err(Error::Validation(
                    "tier_config cannot be null on tiered boards".to_string(),
                ));
            }
            Nullable::Absent => {}
        }
    }

    // Build UPDATE dynamically so Nullable fields can distinguish
    // absent (keep old) from null (clear to NULL) from value (set new).
    let name = input.name.as_deref().unwrap_or(&board.name);
    let sort_direction = input
        .sort_direction
        .as_deref()
        .unwrap_or(&board.sort_direction);
    let tier_config = match &input.tier_config {
        Nullable::Absent => board.tier_config.as_ref(),
        Nullable::Null => None,
        Nullable::Value(v) => Some(v),
    };
    let metadata = match &input.metadata {
        Nullable::Absent => board.metadata.as_ref(),
        Nullable::Null => None,
        Nullable::Value(v) => Some(v),
    };

    let updated = sqlx::query_as::<_, Board>(
        r#"UPDATE boards
           SET name = $1,
               sort_direction = $2,
               tier_config = $3,
               metadata = $4,
               updated_at = now()
           WHERE id = $5
           RETURNING *"#,
    )
    .bind(name)
    .bind(sort_direction)
    .bind(tier_config)
    .bind(metadata)
    .bind(board.id)
    .fetch_one(pool)
    .await?;

    Ok(updated)
}

pub async fn delete(pool: &PgPool, slug: &str) -> Result<()> {
    let result = sqlx::query("DELETE FROM boards WHERE slug = $1")
        .bind(slug)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(Error::NotFound(format!("board '{slug}' not found")));
    }
    Ok(())
}

pub async fn get_by_id(pool: &PgPool, id: Uuid) -> Result<Board> {
    sqlx::query_as::<_, Board>("SELECT * FROM boards WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| Error::NotFound(format!("board with id '{id}' not found")))
}

pub(crate) fn validate_board_type(board_type: &str) -> Result<()> {
    match board_type {
        "ordered" | "scored" | "tiered" => Ok(()),
        _ => Err(Error::Validation(format!(
            "invalid board_type '{board_type}': must be 'ordered', 'scored', or 'tiered'"
        ))),
    }
}

pub(crate) fn validate_sort_direction(sort_direction: &str) -> Result<()> {
    match sort_direction {
        "asc" | "desc" => Ok(()),
        _ => Err(Error::Validation(format!(
            "invalid sort_direction '{sort_direction}': must be 'asc' or 'desc'"
        ))),
    }
}

/// Validate that tier_config has the expected shape:
/// `{ "tiers": [{ "key": "...", "position": N }, ...] }`
/// Each tier must have a string "key" and an integer "position".
fn validate_tier_config(tier_config: Option<&serde_json::Value>) -> Result<()> {
    let tc = match tier_config {
        Some(v) => v,
        None => {
            return Err(Error::Validation(
                "tiered boards must have tier_config with a 'tiers' array".to_string(),
            ));
        }
    };

    let tiers = tc
        .get("tiers")
        .and_then(|t| t.as_array())
        .ok_or_else(|| {
            Error::Validation(
                "tier_config must have a 'tiers' array".to_string(),
            )
        })?;

    if tiers.is_empty() {
        return Err(Error::Validation(
            "tier_config.tiers must not be empty".to_string(),
        ));
    }

    let mut seen_keys = std::collections::HashSet::new();
    for (i, tier) in tiers.iter().enumerate() {
        let key = tier.get("key").and_then(|k| k.as_str()).ok_or_else(|| {
            Error::Validation(format!(
                "tier_config.tiers[{i}] must have a string 'key'"
            ))
        })?;
        if !seen_keys.insert(key) {
            return Err(Error::Validation(format!(
                "tier_config has duplicate tier key '{key}'"
            )));
        }
        let pos = tier
            .get("position")
            .and_then(|p| p.as_i64());
        match pos {
            None => {
                return Err(Error::Validation(format!(
                    "tier_config.tiers[{i}] must have an integer 'position'"
                )));
            }
            Some(v) if !(i32::MIN as i64..=i32::MAX as i64).contains(&v) => {
                return Err(Error::Validation(format!(
                    "tier_config.tiers[{i}] position {v} is out of 32-bit integer range"
                )));
            }
            _ => {}
        }
    }

    Ok(())
}
