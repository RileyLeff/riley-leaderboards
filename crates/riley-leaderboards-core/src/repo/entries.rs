use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{CreateEntry, Entry, Nullable, UpdateEntry};

pub async fn create(pool: &PgPool, board_id: Uuid, input: &CreateEntry) -> Result<Entry> {
    super::validate_slug(&input.slug)?;

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
    let entry = get_by_slug(pool, board_id, slug).await?;

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
    .fetch_one(pool)
    .await?;

    Ok(updated)
}

pub async fn delete(pool: &PgPool, board_id: Uuid, slug: &str) -> Result<()> {
    let entry = get_by_slug(pool, board_id, slug).await?;

    // Reject deletion if the entry has any placements — this would silently
    // mutate historical versions via CASCADE, contradicting version immutability.
    let placement_count: (i64,) =
        sqlx::query_as("SELECT count(*) FROM placements WHERE entry_id = $1")
            .bind(entry.id)
            .fetch_one(pool)
            .await?;
    if placement_count.0 > 0 {
        return Err(Error::Conflict(format!(
            "entry '{slug}' cannot be deleted because it has placements in {} version(s)",
            placement_count.0
        )));
    }

    let result = sqlx::query("DELETE FROM entries WHERE id = $1")
        .bind(entry.id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(Error::NotFound(format!("entry '{slug}' not found")));
    }
    Ok(())
}

pub async fn get_by_id(pool: &PgPool, id: Uuid) -> Result<Entry> {
    sqlx::query_as::<_, Entry>("SELECT * FROM entries WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| Error::NotFound(format!("entry with id '{id}' not found")))
}
