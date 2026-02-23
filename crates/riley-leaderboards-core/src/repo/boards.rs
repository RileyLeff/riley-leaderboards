use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{Board, BoardSummary, CreateBoard, Nullable, UpdateBoard};

pub async fn create(pool: &PgPool, input: &CreateBoard) -> Result<Board> {
    super::validate_slug(&input.slug)?;
    validate_board_type(&input.board_type)?;
    validate_sort_direction(&input.sort_direction)?;

    let board = sqlx::query_as::<_, Board>(
        r#"INSERT INTO boards (slug, name, board_type, sort_direction, tier_config, metadata, accumulative)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING *"#,
    )
    .bind(&input.slug)
    .bind(&input.name)
    .bind(&input.board_type)
    .bind(&input.sort_direction)
    .bind(&input.tier_config)
    .bind(&input.metadata)
    .bind(input.accumulative)
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
    if let Some(ref sd) = input.sort_direction {
        validate_sort_direction(sd)?;
    }

    let board = get_by_slug(pool, slug).await?;

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

fn validate_board_type(board_type: &str) -> Result<()> {
    match board_type {
        "ordered" | "scored" | "tiered" => Ok(()),
        _ => Err(Error::Validation(format!(
            "invalid board_type '{board_type}': must be 'ordered', 'scored', or 'tiered'"
        ))),
    }
}

fn validate_sort_direction(sort_direction: &str) -> Result<()> {
    match sort_direction {
        "asc" | "desc" => Ok(()),
        _ => Err(Error::Validation(format!(
            "invalid sort_direction '{sort_direction}': must be 'asc' or 'desc'"
        ))),
    }
}
