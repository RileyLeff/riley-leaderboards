use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{BoardReference, CreateReference};

pub async fn create(
    pool: &PgPool,
    board_id: Uuid,
    input: &CreateReference,
) -> Result<BoardReference> {
    validate_ref_type(&input.ref_type)?;
    validate_uri(&input.uri)?;
    validate_label(&input.label)?;

    // Resolve pinned_version_number to version ID if provided
    let pinned_version_id = match input.pinned_version_number {
        Some(num) => {
            let version_id: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM versions WHERE board_id = $1 AND version_number = $2",
            )
            .bind(board_id)
            .bind(num)
            .fetch_optional(pool)
            .await?;

            let (id,) = version_id.ok_or_else(|| {
                Error::NotFound(format!("version {num} not found on this board"))
            })?;
            Some(id)
        }
        None => None,
    };

    let reference = sqlx::query_as::<_, BoardReference>(
        r#"WITH inserted AS (
               INSERT INTO board_references (board_id, pinned_version_id, uri, ref_type, label)
               VALUES ($1, $2, $3, $4, $5)
               RETURNING *
           )
           SELECT i.id, i.board_id, i.pinned_version_id,
                  v.version_number AS pinned_version_number,
                  i.uri, i.ref_type, i.label, i.created_at
           FROM inserted i
           LEFT JOIN versions v ON v.id = i.pinned_version_id"#,
    )
    .bind(board_id)
    .bind(pinned_version_id)
    .bind(&input.uri)
    .bind(&input.ref_type)
    .bind(&input.label)
    .fetch_one(pool)
    .await?;

    Ok(reference)
}

pub async fn list(pool: &PgPool, board_id: Uuid) -> Result<Vec<BoardReference>> {
    let refs = sqlx::query_as::<_, BoardReference>(
        r#"SELECT br.id, br.board_id, br.pinned_version_id,
                  v.version_number AS pinned_version_number,
                  br.uri, br.ref_type, br.label, br.created_at
           FROM board_references br
           LEFT JOIN versions v ON v.id = br.pinned_version_id
           WHERE br.board_id = $1
           ORDER BY br.created_at ASC"#,
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;
    Ok(refs)
}

pub async fn list_paginated(
    pool: &PgPool,
    board_id: Uuid,
    params: &crate::models::PaginationParams,
) -> Result<crate::models::PaginatedResponse<BoardReference>> {
    let limit = params.effective_limit();
    let cursor = params.decode_cursor()?;

    let refs = if let Some((ts, id)) = cursor {
        sqlx::query_as::<_, BoardReference>(
            r#"SELECT br.id, br.board_id, br.pinned_version_id,
                      v.version_number AS pinned_version_number,
                      br.uri, br.ref_type, br.label, br.created_at
               FROM board_references br
               LEFT JOIN versions v ON v.id = br.pinned_version_id
               WHERE br.board_id = $1 AND (br.created_at, br.id) > ($2, $3)
               ORDER BY br.created_at ASC, br.id ASC
               LIMIT $4"#,
        )
        .bind(board_id)
        .bind(ts)
        .bind(id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, BoardReference>(
            r#"SELECT br.id, br.board_id, br.pinned_version_id,
                      v.version_number AS pinned_version_number,
                      br.uri, br.ref_type, br.label, br.created_at
               FROM board_references br
               LEFT JOIN versions v ON v.id = br.pinned_version_id
               WHERE br.board_id = $1
               ORDER BY br.created_at ASC, br.id ASC
               LIMIT $2"#,
        )
        .bind(board_id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?
    };

    let has_more = refs.len() as i64 > limit;
    let items: Vec<BoardReference> = refs.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items
            .last()
            .map(|r| crate::models::encode_cursor(&r.created_at, &r.id))
    } else {
        None
    };

    Ok(crate::models::PaginatedResponse { items, next_cursor })
}

pub async fn delete(pool: &PgPool, board_id: Uuid, reference_id: Uuid) -> Result<()> {
    let result =
        sqlx::query("DELETE FROM board_references WHERE id = $1 AND board_id = $2")
            .bind(reference_id)
            .bind(board_id)
            .execute(pool)
            .await?;

    if result.rows_affected() == 0 {
        return Err(Error::NotFound(format!(
            "reference '{reference_id}' not found"
        )));
    }
    Ok(())
}

fn validate_ref_type(ref_type: &str) -> Result<()> {
    match ref_type {
        "embed" | "citation" | "context" => Ok(()),
        _ => Err(Error::Validation(format!(
            "invalid ref_type '{ref_type}': must be 'embed', 'citation', or 'context'"
        ))),
    }
}

fn validate_uri(uri: &str) -> Result<()> {
    if uri.is_empty() {
        return Err(Error::Validation("uri must not be empty".to_string()));
    }
    if uri.len() > 2048 {
        return Err(Error::Validation(
            "uri must not exceed 2048 characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_label(label: &Option<String>) -> Result<()> {
    if let Some(l) = label
        && l.len() > 256 {
            return Err(Error::Validation(
                "label must not exceed 256 characters".to_string(),
            ));
        }
    Ok(())
}
