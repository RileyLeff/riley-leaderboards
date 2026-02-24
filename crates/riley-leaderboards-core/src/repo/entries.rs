use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{CreateEntry, Entry, EntryHistoryItem, Nullable, UpdateEntry};

pub async fn create(pool: &PgPool, board_id: Uuid, input: &CreateEntry) -> Result<Entry> {
    super::validate_slug(&input.slug)?;
    super::validate_name(&input.name)?;

    let entry = sqlx::query_as::<_, Entry>(
        r#"INSERT INTO entries (board_id, slug, name, metadata)
           VALUES ($1, $2, $3, $4)
           RETURNING *"#,
    )
    .bind(board_id)
    .bind(&input.slug)
    .bind(&input.name)
    .bind(&input.metadata)
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            Error::Conflict(format!(
                "entry with slug '{}' already exists on this board",
                input.slug
            ))
        }
        _ => Error::Database(e),
    })?;

    Ok(entry)
}

pub async fn list(pool: &PgPool, board_id: Uuid) -> Result<Vec<Entry>> {
    let entries = sqlx::query_as::<_, Entry>(
        "SELECT * FROM entries WHERE board_id = $1 ORDER BY created_at ASC",
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;
    Ok(entries)
}

pub async fn list_paginated(
    pool: &PgPool,
    board_id: Uuid,
    params: &crate::models::PaginationParams,
) -> Result<crate::models::PaginatedResponse<Entry>> {
    let limit = params.effective_limit();
    let cursor = params.decode_cursor()?;

    let entries = if let Some((ts, id)) = cursor {
        sqlx::query_as::<_, Entry>(
            r#"SELECT * FROM entries
               WHERE board_id = $1 AND (created_at, id) > ($2, $3)
               ORDER BY created_at ASC, id ASC
               LIMIT $4"#,
        )
        .bind(board_id)
        .bind(ts)
        .bind(id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Entry>(
            "SELECT * FROM entries WHERE board_id = $1 ORDER BY created_at ASC, id ASC LIMIT $2",
        )
        .bind(board_id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    };

    let has_more = entries.len() as i64 > limit;
    let items: Vec<Entry> = entries.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items
            .last()
            .map(|e| crate::models::encode_cursor(&e.created_at, &e.id))
    } else {
        None
    };

    Ok(crate::models::PaginatedResponse { items, next_cursor })
}

pub async fn get_by_slug(pool: &PgPool, board_id: Uuid, slug: &str) -> Result<Entry> {
    sqlx::query_as::<_, Entry>(
        "SELECT * FROM entries WHERE board_id = $1 AND slug = $2",
    )
    .bind(board_id)
    .bind(slug)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| Error::NotFound(format!("entry '{slug}' not found")))
}

pub async fn update(
    pool: &PgPool,
    board_id: Uuid,
    slug: &str,
    input: &UpdateEntry,
) -> Result<Entry> {
    if let Some(ref name) = input.name {
        super::validate_name(name)?;
    }

    let mut tx = pool.begin().await?;

    // Lock the row to prevent lost updates from concurrent PATCHes
    let entry = sqlx::query_as::<_, Entry>(
        "SELECT * FROM entries WHERE board_id = $1 AND slug = $2 FOR UPDATE",
    )
    .bind(board_id)
    .bind(slug)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| Error::NotFound(format!("entry '{slug}' not found")))?;

    let name = input.name.as_deref().unwrap_or(&entry.name);
    let metadata = match &input.metadata {
        Nullable::Absent => entry.metadata.as_ref(),
        Nullable::Null => None,
        Nullable::Value(v) => Some(v),
    };

    let updated = sqlx::query_as::<_, Entry>(
        r#"UPDATE entries
           SET name = $1,
               metadata = $2,
               updated_at = now()
           WHERE id = $3
           RETURNING *"#,
    )
    .bind(name)
    .bind(metadata)
    .bind(entry.id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(updated)
}

pub async fn delete(pool: &PgPool, board_id: Uuid, slug: &str) -> Result<()> {
    let mut tx = pool.begin().await?;

    // Look up and lock the entry row to serialize against concurrent version
    // creation that might insert placements for this entry.
    let entry = sqlx::query_as::<_, Entry>(
        "SELECT * FROM entries WHERE board_id = $1 AND slug = $2 FOR UPDATE",
    )
    .bind(board_id)
    .bind(slug)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| Error::NotFound(format!("entry '{slug}' not found")))?;

    // Reject deletion if the entry has any placements — this would silently
    // mutate historical versions via CASCADE, contradicting version immutability.
    let version_count: (i64,) = sqlx::query_as(
        "SELECT count(DISTINCT version_id) FROM placements WHERE entry_id = $1",
    )
    .bind(entry.id)
    .fetch_one(&mut *tx)
    .await?;
    if version_count.0 > 0 {
        return Err(Error::Conflict(format!(
            "entry '{slug}' cannot be deleted because it has placements in {} version(s)",
            version_count.0
        )));
    }

    sqlx::query("DELETE FROM entries WHERE id = $1")
        .bind(entry.id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn get_by_id(pool: &PgPool, id: Uuid) -> Result<Entry> {
    sqlx::query_as::<_, Entry>("SELECT * FROM entries WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| Error::NotFound(format!("entry with id '{id}' not found")))
}

pub async fn history(
    pool: &PgPool,
    board_id: Uuid,
    entry_slug: &str,
    limit: i64,
) -> Result<Vec<EntryHistoryItem>> {
    let entry = get_by_slug(pool, board_id, entry_slug).await?;

    let items = sqlx::query_as::<_, EntryHistoryItem>(
        r#"SELECT v.version_number, p.position, p.score, p.tier,
                  p.metadata, v.created_at
           FROM placements p
           JOIN versions v ON v.id = p.version_id
           WHERE p.entry_id = $1
           ORDER BY v.version_number ASC
           LIMIT $2"#,
    )
    .bind(entry.id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(items)
}
