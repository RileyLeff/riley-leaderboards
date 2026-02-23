//! Sync execution: compare parsed board files against DB state, create/update as needed.

use std::path::Path;

use sqlx::PgPool;
use tracing::{info, warn};

use crate::error::{Error, Result};
use crate::models::{
    CreateBoard, CreateEntry, CreatePlacement, CreateVersion, Nullable, UpdateBoard, UpdateEntry,
};
use crate::repo::{boards, entries, versions};

use super::parse::{self, ParsedBoard, toml_to_json};

/// Result of syncing a single board.
#[derive(Debug)]
pub struct BoardSyncResult {
    pub slug: String,
    pub action: SyncAction,
}

#[derive(Debug)]
pub enum SyncAction {
    Created { version_number: i32 },
    Updated { version_number: i32 },
    NoChange,
    Skipped { reason: String },
    Failed { error: String },
}

/// Sync all boards from a directory against the database.
pub async fn sync_dir(
    pool: &PgPool,
    dir: &Path,
    note: Option<&str>,
) -> Result<Vec<BoardSyncResult>> {
    let parsed = parse::parse_boards_dir(dir)?;

    if parsed.is_empty() {
        info!("no board directories found in {}", dir.display());
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for board in parsed {
        let slug = board.slug.clone();
        match sync_board(pool, board, note).await {
            Ok(result) => results.push(result),
            Err(e) => {
                warn!("failed to sync board '{slug}': {e}");
                results.push(BoardSyncResult {
                    slug,
                    action: SyncAction::Failed {
                        error: e.to_string(),
                    },
                });
            }
        }
    }

    Ok(results)
}

/// Sync a single parsed board against the database.
async fn sync_board(
    pool: &PgPool,
    parsed: ParsedBoard,
    note: Option<&str>,
) -> Result<BoardSyncResult> {
    // Skip accumulative boards — they're managed via score submission, not files
    if parsed.board.accumulative {
        return Ok(BoardSyncResult {
            slug: parsed.slug,
            action: SyncAction::Skipped {
                reason: "accumulative boards are managed via score submission, not file sync"
                    .to_string(),
            },
        });
    }

    // Convert tier config from TOML to JSON
    let tier_config = if !parsed.board.tiers.is_empty() {
        let tiers: Vec<serde_json::Value> = parsed
            .board
            .tiers
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut obj = serde_json::Map::new();
                obj.insert("key".to_string(), serde_json::Value::String(t.key.clone()));
                obj.insert("position".to_string(), serde_json::json!((i + 1) as i32));
                if let Some(ref label) = t.label {
                    obj.insert(
                        "label".to_string(),
                        serde_json::Value::String(label.clone()),
                    );
                }
                serde_json::Value::Object(obj)
            })
            .collect();
        Some(serde_json::json!({ "tiers": tiers }))
    } else {
        None
    };

    let metadata = parsed.board.metadata.as_ref().map(toml_to_json);

    // Create or update the board, tracking if ranking-critical config changed
    let mut ranking_config_changed = false;
    let board = match boards::get_by_slug(pool, &parsed.slug).await {
        Ok(existing) => {
            // Warn if board_type in TOML differs from DB (board_type is immutable)
            if existing.board_type != parsed.board.board_type {
                warn!(
                    "board '{}': TOML board_type '{}' differs from DB '{}' (board_type is immutable, ignoring)",
                    parsed.slug, parsed.board.board_type, existing.board_type
                );
            }

            // Update board metadata if changed
            let name_changed = existing.name != parsed.board.name;
            let sd_changed = existing.sort_direction != parsed.board.sort_direction;
            let tc_changed = existing.tier_config != tier_config;
            let meta_changed = existing.metadata != metadata;

            // sort_direction and tier_config affect ranking — force a new version
            if sd_changed || tc_changed {
                ranking_config_changed = true;
            }

            if name_changed || sd_changed || tc_changed || meta_changed {
                let update = UpdateBoard {
                    name: if name_changed {
                        Some(parsed.board.name.clone())
                    } else {
                        None
                    },
                    sort_direction: if sd_changed {
                        Some(parsed.board.sort_direction.clone())
                    } else {
                        None
                    },
                    tier_config: if tc_changed {
                        match &tier_config {
                            Some(v) => crate::models::Nullable::Value(v.clone()),
                            None => crate::models::Nullable::Null,
                        }
                    } else {
                        crate::models::Nullable::Absent
                    },
                    metadata: if meta_changed {
                        match &metadata {
                            Some(v) => crate::models::Nullable::Value(v.clone()),
                            None => crate::models::Nullable::Null,
                        }
                    } else {
                        crate::models::Nullable::Absent
                    },
                };
                boards::update(pool, &parsed.slug, &update).await?
            } else {
                existing
            }
        }
        Err(Error::NotFound(_)) => {
            let create = CreateBoard {
                slug: parsed.slug.clone(),
                name: parsed.board.name.clone(),
                board_type: parsed.board.board_type.clone(),
                sort_direction: parsed.board.sort_direction.clone(),
                tier_config,
                metadata,
                accumulative: false,
            };
            boards::create(pool, &create).await?
        }
        Err(e) => return Err(e),
    };

    // If no entries in the file, nothing to version
    if parsed.entries.is_empty() {
        info!("board '{}': no entries in rankings.toml, skipping version creation", parsed.slug);
        return Ok(BoardSyncResult {
            slug: parsed.slug,
            action: SyncAction::NoChange,
        });
    }

    // Ensure all entries exist and update metadata if changed
    let existing_entries = entries::list(pool, board.id).await?;

    // Build a lookup map for O(1) entry access
    let existing_entry_map: std::collections::HashMap<&str, &crate::models::Entry> =
        existing_entries
            .iter()
            .map(|e| (e.slug.as_str(), e))
            .collect();

    for entry in &parsed.entries {
        match existing_entry_map.get(entry.slug.as_str()) {
            None => {
                let create = CreateEntry {
                    slug: entry.slug.clone(),
                    name: entry.name.clone(),
                    metadata: entry.metadata.as_ref().map(toml_to_json),
                };
                entries::create(pool, board.id, &create).await?;
            }
            Some(existing) => {
                // Update existing entry if name or metadata changed
                let new_metadata = entry.metadata.as_ref().map(toml_to_json);
                let name_changed = existing.name != entry.name;
                let meta_changed = existing.metadata != new_metadata;
                if name_changed || meta_changed {
                    let update = UpdateEntry {
                        name: if name_changed {
                            Some(entry.name.clone())
                        } else {
                            None
                        },
                        metadata: if meta_changed {
                            match new_metadata {
                                Some(v) => Nullable::Value(v),
                                None => Nullable::Null,
                            }
                        } else {
                            Nullable::Absent
                        },
                    };
                    entries::update(pool, board.id, &entry.slug, &update).await?;
                }
            }
        }
    }

    // Build placements from the parsed entries
    let placements: Vec<CreatePlacement> = parsed
        .entries
        .iter()
        .map(|e| CreatePlacement {
            entry_slug: e.slug.clone(),
            position: e.position,
            score: e.score,
            tier: e.tier.clone(),
            metadata: e.metadata.as_ref().map(toml_to_json),
        })
        .collect();

    // Check if a new version is needed by diffing against the latest.
    // Force a new version if ranking-critical config (sort_direction, tier_config) changed.
    let needs_version = if ranking_config_changed {
        info!(
            "board '{}': ranking config changed, forcing new version",
            parsed.slug
        );
        true
    } else {
        match versions::get_latest(pool, board.id).await {
            Ok(latest) => {
                placements_changed(&board.board_type, &latest.placements, &placements)
            }
            Err(Error::NotFound(_)) => true, // No versions yet
            Err(e) => return Err(e),
        }
    };

    if !needs_version {
        info!("board '{}': no changes detected, skipping version creation", parsed.slug);
        return Ok(BoardSyncResult {
            slug: parsed.slug,
            action: SyncAction::NoChange,
        });
    }

    // Create the new version
    let version_note = note.unwrap_or("Synced from file");
    let create_version = CreateVersion {
        note: Some(version_note.to_string()),
        placements,
    };
    let version = versions::create(pool, &board, &create_version).await?;
    let vnum = version.version.version_number;

    // Determine if this was a create (v1) or update (v2+)
    let action = if vnum == 1 {
        info!("board '{}': created version {vnum}", parsed.slug);
        SyncAction::Created {
            version_number: vnum,
        }
    } else {
        info!("board '{}': updated to version {vnum}", parsed.slug);
        SyncAction::Updated {
            version_number: vnum,
        }
    };

    Ok(BoardSyncResult {
        slug: parsed.slug,
        action,
    })
}

/// Compare current placements against proposed placements to detect changes.
fn placements_changed(
    board_type: &str,
    current: &[crate::models::PlacementWithEntry],
    proposed: &[crate::models::CreatePlacement],
) -> bool {
    if current.len() != proposed.len() {
        return true;
    }

    // Build a lookup of current placements by entry slug
    let current_map: std::collections::HashMap<&str, &crate::models::PlacementWithEntry> = current
        .iter()
        .map(|p| (p.entry_slug.as_str(), p))
        .collect();

    for (idx, p) in proposed.iter().enumerate() {
        match current_map.get(p.entry_slug.as_str()) {
            None => return true, // New entry
            Some(current_p) => {
                if p.position.is_some() {
                    // Explicit position set — compare directly
                    if current_p.position != p.position {
                        return true;
                    }
                } else if board_type == "ordered" {
                    // Ordered board with implicit positions: derive from array
                    // index (1-based) and compare against the DB's stored position
                    let derived_position = Some((idx + 1) as i32);
                    if current_p.position != derived_position {
                        return true;
                    }
                }
                // For scored/tiered boards with no explicit position, skip
                // position comparison (positions are derived from scores/tiers)

                if current_p.score != p.score {
                    return true;
                }
                if current_p.tier != p.tier {
                    return true;
                }
                // Check metadata
                let proposed_meta = p.metadata.as_ref();
                if current_p.metadata.as_ref() != proposed_meta {
                    return true;
                }
            }
        }
    }

    false
}
