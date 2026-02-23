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
        r#"INSERT INTO board_references (board_id, pinned_version_id, uri, ref_type, label)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING *"#,
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
        "SELECT * FROM board_references WHERE board_id = $1 ORDER BY created_at ASC",
    )
    .bind(board_id)
    .fetch_all(pool)
    .await?;
    Ok(refs)
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
