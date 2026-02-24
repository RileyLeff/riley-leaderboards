use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::{Error, Result};
use crate::models::{Board, PlacementWithEntry, Version};

/// Full export of a board: board metadata + all versions with placements.
#[derive(Debug, Serialize, Deserialize)]
pub struct BoardExport {
    pub board: BoardExportMeta,
    pub versions: Vec<VersionExport>,
}

/// Board metadata for export (excludes internal id/timestamps).
#[derive(Debug, Serialize, Deserialize)]
pub struct BoardExportMeta {
    pub slug: String,
    pub name: String,
    pub board_type: String,
    pub sort_direction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier_config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub accumulative: bool,
    #[serde(default)]
    pub realtime: bool,
    #[serde(default)]
    pub clear_on_snapshot: bool,
}

impl From<&Board> for BoardExportMeta {
    fn from(b: &Board) -> Self {
        Self {
            slug: b.slug.clone(),
            name: b.name.clone(),
            board_type: b.board_type.clone(),
            sort_direction: b.sort_direction.clone(),
            tier_config: b.tier_config.clone(),
            metadata: b.metadata.clone(),
            accumulative: b.accumulative,
            realtime: b.realtime,
            clear_on_snapshot: b.clear_on_snapshot,
        }
    }
}

/// A version in the export format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionExport {
    pub version_number: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub placements: Vec<PlacementExport>,
}

/// A placement in the export format (references entry by slug, not ID).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementExport {
    pub entry_slug: String,
    pub entry_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl From<&PlacementWithEntry> for PlacementExport {
    fn from(p: &PlacementWithEntry) -> Self {
        Self {
            entry_slug: p.entry_slug.clone(),
            entry_name: p.entry_name.clone(),
            position: p.position,
            score: p.score,
            tier: p.tier.clone(),
            metadata: p.metadata.clone(),
        }
    }
}

/// Export a board and all its versions with placements as a portable JSON structure.
///
/// Uses a batch query to fetch all placements across all versions in a single round-trip,
/// avoiding the N+1 pattern of fetching placements per version.
pub async fn export_board(pool: &PgPool, slug: &str) -> Result<BoardExport> {
    use std::collections::HashMap;

    let board = super::boards::get_by_slug(pool, slug).await?;
    let versions = super::versions::list(pool, board.id).await?;

    // Fetch all placements for all versions in one query, grouped by version_id
    let all_placements = sqlx::query_as::<_, PlacementWithEntry>(
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
           WHERE v.board_id = $1
           ORDER BY v.version_number ASC,
                    COALESCE(tc.tier_ord, 2147483647) ASC,
                    COALESCE(p.position, 2147483647) ASC,
                    e.name ASC"#,
    )
    .bind(board.id)
    .fetch_all(pool)
    .await?;

    // Group placements by version_id
    let mut placements_by_version: HashMap<uuid::Uuid, Vec<PlacementExport>> = HashMap::new();
    for p in &all_placements {
        placements_by_version
            .entry(p.version_id)
            .or_default()
            .push(PlacementExport::from(p));
    }

    // Build version exports sorted by version_number ascending
    let mut version_exports: Vec<VersionExport> = versions
        .iter()
        .map(|v| VersionExport {
            version_number: v.version_number,
            note: v.note.clone(),
            metadata: v.metadata.clone(),
            placements: placements_by_version.remove(&v.id).unwrap_or_default(),
        })
        .collect();
    version_exports.sort_by_key(|v| v.version_number);

    Ok(BoardExport {
        board: BoardExportMeta::from(&board),
        versions: version_exports,
    })
}

/// Result of importing a board — the board slug and all version numbers created.
pub struct ImportResult {
    pub board_slug: String,
    pub version_numbers: Vec<i32>,
}

/// Import a board from an export. Creates the board, entries, and all versions
/// inside a single transaction (atomic — all-or-nothing).
///
/// Uses direct SQL inserts for versions/placements to bypass the accumulative
/// board check, but validates placement data before inserting.
///
/// Fails if a board with the same slug already exists.
pub async fn import_board(pool: &PgPool, export: &BoardExport) -> Result<ImportResult> {
    use crate::models::{CreateBoard, CreatePlacement};
    use std::collections::HashMap;

    // Pre-validate all version placements before starting the transaction
    let board_meta = &export.board;
    for v in &export.versions {
        let placements: Vec<CreatePlacement> = v
            .placements
            .iter()
            .map(|p| CreatePlacement {
                entry_slug: p.entry_slug.clone(),
                position: p.position,
                score: p.score,
                tier: p.tier.clone(),
                metadata: p.metadata.clone(),
            })
            .collect();

        // Build a temporary Board-like struct for validation
        let temp_board = crate::models::Board {
            id: uuid::Uuid::nil(),
            slug: board_meta.slug.clone(),
            name: board_meta.name.clone(),
            board_type: board_meta.board_type.clone(),
            sort_direction: board_meta.sort_direction.clone(),
            tier_config: board_meta.tier_config.clone(),
            metadata: board_meta.metadata.clone(),
            accumulative: board_meta.accumulative,
            realtime: board_meta.realtime,
            clear_on_snapshot: board_meta.clear_on_snapshot,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        super::versions::validate_placements_for_import(&temp_board, &placements)?;
    }

    let mut tx = pool.begin().await.map_err(Error::Database)?;

    // Create the board
    let create_board = CreateBoard {
        slug: export.board.slug.clone(),
        name: export.board.name.clone(),
        board_type: export.board.board_type.clone(),
        sort_direction: export.board.sort_direction.clone(),
        tier_config: export.board.tier_config.clone(),
        metadata: export.board.metadata.clone(),
        accumulative: export.board.accumulative,
        realtime: export.board.realtime,
        clear_on_snapshot: export.board.clear_on_snapshot,
    };

    super::validate_slug(&create_board.slug)?;
    super::validate_name(&create_board.name)?;
    super::boards::validate_board_type(&create_board.board_type)?;
    super::boards::validate_sort_direction(&create_board.sort_direction)?;

    // Cross-field constraints (same as boards::create)
    if create_board.accumulative && create_board.board_type != "scored" {
        return Err(Error::Validation(
            "accumulative boards must have board_type 'scored'".to_string(),
        ));
    }
    if create_board.realtime && !(create_board.accumulative && create_board.board_type == "scored")
    {
        return Err(Error::Validation(
            "realtime boards must be accumulative and scored".to_string(),
        ));
    }
    if create_board.clear_on_snapshot && !create_board.realtime {
        return Err(Error::Validation(
            "clear_on_snapshot requires realtime to be true".to_string(),
        ));
    }

    let board = sqlx::query_as::<_, crate::models::Board>(
        r#"INSERT INTO boards (slug, name, board_type, sort_direction, tier_config, metadata, accumulative, realtime, clear_on_snapshot)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
           RETURNING *"#,
    )
    .bind(&create_board.slug)
    .bind(&create_board.name)
    .bind(&create_board.board_type)
    .bind(&create_board.sort_direction)
    .bind(&create_board.tier_config)
    .bind(&create_board.metadata)
    .bind(create_board.accumulative)
    .bind(create_board.realtime)
    .bind(create_board.clear_on_snapshot)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            Error::Conflict(format!(
                "board with slug '{}' already exists",
                create_board.slug
            ))
        }
        _ => Error::Database(e),
    })?;

    // Collect all unique entries across all versions (use last-seen name),
    // create them, and build slug->id map
    let mut entry_names: HashMap<String, String> = HashMap::new();
    for v in &export.versions {
        for p in &v.placements {
            entry_names.insert(p.entry_slug.clone(), p.entry_name.clone());
        }
    }

    let mut entry_ids: HashMap<String, uuid::Uuid> = HashMap::new();
    for (slug, name) in &entry_names {
        super::validate_slug(slug)?;
        super::validate_name(name)?;
        let entry = sqlx::query_as::<_, crate::models::Entry>(
            r#"INSERT INTO entries (board_id, slug, name)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(board.id)
        .bind(slug)
        .bind(name)
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::Database)?;
        entry_ids.insert(slug.clone(), entry.id);
    }

    // Create versions in order via direct SQL (bypasses accumulative check)
    let mut sorted_versions = export.versions.clone();
    sorted_versions.sort_by_key(|v| v.version_number);

    for v in &sorted_versions {
        let version = sqlx::query_as::<_, Version>(
            r#"INSERT INTO versions (board_id, version_number, note, metadata)
               VALUES ($1, $2, $3, $4)
               RETURNING *"#,
        )
        .bind(board.id)
        .bind(v.version_number)
        .bind(&v.note)
        .bind(&v.metadata)
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::Database)?;

        for p in &v.placements {
            let entry_id = entry_ids.get(&p.entry_slug).ok_or_else(|| {
                Error::Validation(format!("unknown entry slug '{}' in export", p.entry_slug))
            })?;

            sqlx::query(
                r#"INSERT INTO placements (version_id, entry_id, position, score, tier, metadata)
                   VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(version.id)
            .bind(entry_id)
            .bind(p.position)
            .bind(p.score)
            .bind(&p.tier)
            .bind(&p.metadata)
            .execute(&mut *tx)
            .await
            .map_err(Error::Database)?;
        }

        // Derive positions for scored boards (positions are based on score + sort_direction)
        if board.board_type == "scored" {
            super::versions::derive_scored_positions(
                &mut tx,
                version.id,
                &board.sort_direction,
            )
            .await?;
        }
    }

    let version_numbers: Vec<i32> = sorted_versions.iter().map(|v| v.version_number).collect();

    tx.commit().await.map_err(Error::Database)?;

    Ok(ImportResult {
        board_slug: board.slug.clone(),
        version_numbers,
    })
}
