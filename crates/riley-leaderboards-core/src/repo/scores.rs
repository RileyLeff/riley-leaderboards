use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{
    AccumulatedScore, Board, PlacementWithEntry, SubmitScore, Version, VersionWithPlacements,
};

/// Submit (upsert) a score for an accumulative board.
/// Creates the entry if it doesn't exist; updates name on re-submission.
pub async fn submit(
    pool: &PgPool,
    board: &Board,
    input: &SubmitScore,
) -> Result<AccumulatedScore> {
    if !board.accumulative {
        return Err(Error::Validation(
            "score submission is only allowed on accumulative boards".to_string(),
        ));
    }
    if board.board_type != "scored" {
        return Err(Error::Validation(
            "score submission is only allowed on scored boards".to_string(),
        ));
    }
    if input.score.is_nan() || input.score.is_infinite() {
        return Err(Error::Validation(
            "score must be a finite number".to_string(),
        ));
    }
    super::validate_slug(&input.entry_slug)?;
    super::validate_name(&input.entry_name)?;

    let mut tx = pool.begin().await?;

    // Upsert the entry (create if not exists, update name on re-submission)
    let entry_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO entries (board_id, slug, name)
           VALUES ($1, $2, $3)
           ON CONFLICT (board_id, slug) DO UPDATE SET name = $3, updated_at = now()
           RETURNING id"#,
    )
    .bind(board.id)
    .bind(&input.entry_slug)
    .bind(&input.entry_name)
    .fetch_one(&mut *tx)
    .await?;

    // Upsert the accumulated score
    let score = sqlx::query_as::<_, AccumulatedScore>(
        r#"INSERT INTO accumulated_scores (board_id, entry_id, score)
           VALUES ($1, $2, $3)
           ON CONFLICT (board_id, entry_id) DO UPDATE SET score = $3, submitted_at = now()
           RETURNING *"#,
    )
    .bind(board.id)
    .bind(entry_id)
    .bind(input.score)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(score)
}

/// Snapshot current accumulated scores as a new version.
pub async fn snapshot(
    pool: &PgPool,
    board: &Board,
    note: Option<&str>,
) -> Result<VersionWithPlacements> {
    if !board.accumulative {
        return Err(Error::Validation(
            "snapshot is only allowed on accumulative boards".to_string(),
        ));
    }
    if board.board_type != "scored" {
        return Err(Error::Validation(
            "snapshot is only allowed on scored boards".to_string(),
        ));
    }

    let mut tx = pool.begin().await?;

    // Lock the board row to serialize concurrent snapshots
    let board = sqlx::query_as::<_, Board>("SELECT * FROM boards WHERE id = $1 FOR UPDATE")
        .bind(board.id)
        .fetch_one(&mut *tx)
        .await?;

    // Read accumulated scores with entry info
    let scores: Vec<(Uuid, f64, String, String)> = sqlx::query_as(
        r#"SELECT a.entry_id, a.score, e.slug, e.name
           FROM accumulated_scores a
           JOIN entries e ON e.id = a.entry_id
           WHERE a.board_id = $1"#,
    )
    .bind(board.id)
    .fetch_all(&mut *tx)
    .await?;

    if scores.is_empty() {
        return Err(Error::Validation(
            "no accumulated scores to snapshot".to_string(),
        ));
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
        r#"INSERT INTO versions (board_id, version_number, note)
           VALUES ($1, $2, $3)
           RETURNING *"#,
    )
    .bind(board.id)
    .bind(next_number)
    .bind(note)
    .fetch_one(&mut *tx)
    .await?;

    // Insert placements from accumulated scores (no positions yet)
    for (entry_id, score, _slug, _name) in &scores {
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

    // Derive positions from scores (reuse shared logic)
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

    tx.commit().await?;

    Ok(VersionWithPlacements {
        version,
        placements,
    })
}
