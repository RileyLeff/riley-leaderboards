//! Parse board.toml and rankings.toml files into intermediate structs.

use std::path::Path;

use serde::Deserialize;
use tracing;

use crate::error::{Error, Result};

/// Parsed board.toml
#[derive(Debug, Deserialize)]
pub struct BoardToml {
    pub name: String,
    pub board_type: String,
    #[serde(default = "default_sort_direction")]
    pub sort_direction: String,
    #[serde(default)]
    pub accumulative: bool,
    #[serde(default)]
    pub tiers: Vec<TierToml>,
    pub metadata: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
pub struct TierToml {
    pub key: String,
    pub label: Option<String>,
}

fn default_sort_direction() -> String {
    "desc".to_string()
}

/// Parsed rankings.toml
#[derive(Debug, Deserialize)]
pub struct RankingsToml {
    #[serde(default)]
    pub entries: Vec<EntryToml>,
    pub version_metadata: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
pub struct EntryToml {
    pub slug: String,
    pub name: String,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    pub metadata: Option<toml::Value>,
}

/// A fully parsed board directory (board.toml + rankings.toml).
#[derive(Debug)]
pub struct ParsedBoard {
    pub slug: String,
    pub board: BoardToml,
    pub entries: Vec<EntryToml>,
    pub version_metadata: Option<serde_json::Value>,
}

/// Parse a single board directory (must contain board.toml).
/// rankings.toml is optional — if missing, the board is synced with no entries.
pub fn parse_board_dir(dir: &Path) -> Result<ParsedBoard> {
    let slug = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Validation(format!("invalid board directory: {}", dir.display())))?
        .to_string();
    crate::repo::validate_slug(&slug)?;

    let board_path = dir.join("board.toml");
    let board_str = std::fs::read_to_string(&board_path)
        .map_err(|e| Error::Validation(format!("failed to read {}: {e}", board_path.display())))?;
    let board: BoardToml = toml::from_str(&board_str)
        .map_err(|e| Error::Validation(format!("failed to parse {}: {e}", board_path.display())))?;

    let rankings_path = dir.join("rankings.toml");
    let (entries, version_metadata) = if rankings_path.exists() {
        let rankings_str = std::fs::read_to_string(&rankings_path).map_err(|e| {
            Error::Validation(format!("failed to read {}: {e}", rankings_path.display()))
        })?;
        let rankings: RankingsToml = toml::from_str(&rankings_str).map_err(|e| {
            Error::Validation(format!("failed to parse {}: {e}", rankings_path.display()))
        })?;
        let vm = rankings.version_metadata.as_ref().map(toml_to_json);
        (rankings.entries, vm)
    } else {
        (Vec::new(), None)
    };

    Ok(ParsedBoard {
        slug,
        board,
        entries,
        version_metadata,
    })
}

/// Scan a directory for board subdirectories and parse each one.
/// A subdirectory is considered a board if it contains board.toml.
pub fn parse_boards_dir(dir: &Path) -> Result<Vec<ParsedBoard>> {
    if !dir.is_dir() {
        return Err(Error::Validation(format!(
            "{} is not a directory",
            dir.display()
        )));
    }

    let mut boards = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| Error::Validation(format!("failed to read directory {}: {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() && path.join("board.toml").exists() {
            match parse_board_dir(&path) {
                Ok(board) => boards.push(board),
                Err(e) => {
                    tracing::warn!("skipping board directory {}: {e}", path.display());
                }
            }
        }
    }

    Ok(boards)
}

/// Convert a TOML value to a serde_json::Value for storage in JSONB columns.
pub fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::json!(*i),
        toml::Value::Float(f) => serde_json::json!(*f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => {
            let map: serde_json::Map<String, serde_json::Value> = table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}
