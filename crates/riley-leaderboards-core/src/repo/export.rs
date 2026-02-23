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
        }
    }
}

/// A version in the export format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionExport {
    pub version_number: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
pub async fn export_board(pool: &PgPool, slug: &str) -> Result<BoardExport> {
    let board = super::boards::get_by_slug(pool, slug).await?;
    let versions = super::versions::list(pool, board.id).await?;

    let mut version_exports = Vec::with_capacity(versions.len());
    for v in &versions {
        let vwp = super::versions::get_by_number(pool, board.id, v.version_number).await?;
        version_exports.push(VersionExport {
            version_number: vwp.version.version_number,
            note: vwp.version.note.clone(),
            placements: vwp.placements.iter().map(PlacementExport::from).collect(),
        });
    }

    // Sort by version number ascending for readability
    version_exports.sort_by_key(|v| v.version_number);

    Ok(BoardExport {
        board: BoardExportMeta::from(&board),
        versions: version_exports,
    })
}

/// Import a board from an export. Creates the board, entries, and all versions.
///
/// Uses direct SQL inserts for versions/placements to bypass the accumulative
/// board check (the export is trusted data). If import fails partway through,
/// use `delete-board` to clean up and retry.
///
/// Fails if a board with the same slug already exists.
pub async fn import_board(pool: &PgPool, export: &BoardExport) -> Result<()> {
    use crate::models::CreateBoard;
    use std::collections::HashMap;

    // Create the board via the normal path (validates slug, type, etc.)
    let create_board = CreateBoard {
        slug: export.board.slug.clone(),
        name: export.board.name.clone(),
        board_type: export.board.board_type.clone(),
        sort_direction: export.board.sort_direction.clone(),
        tier_config: export.board.tier_config.clone(),
        metadata: export.board.metadata.clone(),
        accumulative: export.board.accumulative,
    };
    let board = super::boards::create(pool, &create_board).await?;

    // Collect all unique entries across all versions, create them, and build slug->id map
    let mut entry_ids: HashMap<String, uuid::Uuid> = HashMap::new();
    for v in &export.versions {
        for p in &v.placements {
            if !entry_ids.contains_key(&p.entry_slug) {
                let entry = crate::models::CreateEntry {
                    slug: p.entry_slug.clone(),
                    name: p.entry_name.clone(),
                    metadata: None,
                };
                let created = super::entries::create(pool, board.id, &entry).await?;
                entry_ids.insert(p.entry_slug.clone(), created.id);
            }
        }
    }

    // Create versions in order via direct SQL (bypasses accumulative check)
    let mut sorted_versions = export.versions.clone();
    sorted_versions.sort_by_key(|v| v.version_number);

    for v in &sorted_versions {
        let version = sqlx::query_as::<_, Version>(
            r#"INSERT INTO versions (board_id, version_number, note)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(board.id)
        .bind(v.version_number)
        .bind(&v.note)
        .fetch_one(pool)
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
            .execute(pool)
            .await
            .map_err(Error::Database)?;
        }
    }

    Ok(())
}
