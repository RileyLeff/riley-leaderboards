use sqlx::PgPool;

use crate::error::{Error, Result};
use crate::models::{
    AddBoardToCollection, Collection, CollectionBoardEntry, CollectionWithBoards, CreateCollection,
    Nullable, PaginatedResponse, PaginationParams, UpdateCollection, encode_cursor,
};

pub async fn create(pool: &PgPool, input: &CreateCollection) -> Result<Collection> {
    super::validate_slug(&input.slug)?;
    super::validate_name(&input.name)?;

    let collection = sqlx::query_as::<_, Collection>(
        r#"INSERT INTO collections (slug, name, metadata)
           VALUES ($1, $2, $3)
           RETURNING *"#,
    )
    .bind(&input.slug)
    .bind(&input.name)
    .bind(&input.metadata)
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => Error::Conflict(format!(
            "collection with slug '{}' already exists",
            input.slug
        )),
        _ => Error::Database(e),
    })?;

    Ok(collection)
}

pub async fn list(pool: &PgPool) -> Result<Vec<Collection>> {
    let collections =
        sqlx::query_as::<_, Collection>("SELECT * FROM collections ORDER BY created_at DESC")
            .fetch_all(pool)
            .await?;
    Ok(collections)
}

pub async fn list_paginated(
    pool: &PgPool,
    params: &PaginationParams,
) -> Result<PaginatedResponse<Collection>> {
    let limit = params.effective_limit();
    let cursor = params.decode_cursor()?;

    let collections = if let Some((ts, id)) = cursor {
        sqlx::query_as::<_, Collection>(
            r#"SELECT * FROM collections
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
        sqlx::query_as::<_, Collection>(
            "SELECT * FROM collections ORDER BY created_at DESC, id DESC LIMIT $1",
        )
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    };

    let has_more = collections.len() as i64 > limit;
    let items: Vec<Collection> = collections.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items.last().map(|c| encode_cursor(&c.created_at, &c.id))
    } else {
        None
    };

    Ok(PaginatedResponse { items, next_cursor })
}

pub async fn get_by_slug(pool: &PgPool, slug: &str) -> Result<Collection> {
    sqlx::query_as::<_, Collection>("SELECT * FROM collections WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| Error::NotFound(format!("collection '{slug}' not found")))
}

pub async fn get_with_boards(pool: &PgPool, slug: &str) -> Result<CollectionWithBoards> {
    let collection = get_by_slug(pool, slug).await?;

    let boards = sqlx::query_as::<_, CollectionBoardEntry>(
        r#"SELECT b.slug, b.name, b.board_type, cb.display_order,
                  (SELECT MAX(v.version_number) FROM versions v WHERE v.board_id = b.id) AS latest_version
           FROM collection_boards cb
           JOIN boards b ON b.id = cb.board_id
           WHERE cb.collection_id = $1
           ORDER BY cb.display_order ASC, b.name ASC"#,
    )
    .bind(collection.id)
    .fetch_all(pool)
    .await?;

    Ok(CollectionWithBoards { collection, boards })
}

pub async fn update(pool: &PgPool, slug: &str, input: &UpdateCollection) -> Result<Collection> {
    if let Some(ref name) = input.name {
        super::validate_name(name)?;
    }

    let mut tx = pool.begin().await?;

    // Lock the row to prevent lost updates from concurrent PATCHes
    let collection =
        sqlx::query_as::<_, Collection>("SELECT * FROM collections WHERE slug = $1 FOR UPDATE")
            .bind(slug)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| Error::NotFound(format!("collection '{slug}' not found")))?;

    let name = input.name.as_deref().unwrap_or(&collection.name);
    let metadata = match &input.metadata {
        Nullable::Absent => collection.metadata.as_ref(),
        Nullable::Null => None,
        Nullable::Value(v) => Some(v),
    };

    let updated = sqlx::query_as::<_, Collection>(
        r#"UPDATE collections
           SET name = $1,
               metadata = $2,
               updated_at = now()
           WHERE id = $3
           RETURNING *"#,
    )
    .bind(name)
    .bind(metadata)
    .bind(collection.id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(updated)
}

pub async fn delete(pool: &PgPool, slug: &str) -> Result<()> {
    let result = sqlx::query("DELETE FROM collections WHERE slug = $1")
        .bind(slug)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(Error::NotFound(format!("collection '{slug}' not found")));
    }
    Ok(())
}

// ── Board membership ──────────────────────────────────────────────────────

pub async fn add_board(
    pool: &PgPool,
    collection_slug: &str,
    input: &AddBoardToCollection,
) -> Result<()> {
    let collection = get_by_slug(pool, collection_slug).await?;
    let board = crate::repo::boards::get_by_slug(pool, &input.board_slug).await?;

    sqlx::query(
        r#"INSERT INTO collection_boards (collection_id, board_id, display_order)
           VALUES ($1, $2, $3)"#,
    )
    .bind(collection.id)
    .bind(board.id)
    .bind(input.display_order)
    .execute(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => Error::Conflict(format!(
            "board '{}' is already in collection '{}'",
            input.board_slug, collection_slug
        )),
        sqlx::Error::Database(db_err) if db_err.is_foreign_key_violation() => {
            Error::NotFound(format!(
                "collection '{}' or board '{}' not found",
                collection_slug, input.board_slug
            ))
        }
        _ => Error::Database(e),
    })?;

    Ok(())
}

pub async fn remove_board(pool: &PgPool, collection_slug: &str, board_slug: &str) -> Result<()> {
    let collection = get_by_slug(pool, collection_slug).await?;
    let board = crate::repo::boards::get_by_slug(pool, board_slug).await?;

    let result =
        sqlx::query("DELETE FROM collection_boards WHERE collection_id = $1 AND board_id = $2")
            .bind(collection.id)
            .bind(board.id)
            .execute(pool)
            .await?;

    if result.rows_affected() == 0 {
        return Err(Error::NotFound(format!(
            "board '{board_slug}' is not in collection '{collection_slug}'"
        )));
    }
    Ok(())
}
