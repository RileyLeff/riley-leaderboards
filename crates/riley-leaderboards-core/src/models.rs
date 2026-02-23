use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

// ── Nullable (for PATCH semantics) ─────────────────────────────────────────

/// Three-state type for PATCH operations:
/// - `None` = field absent from JSON → keep existing value
/// - `Some(None)` = field present as `null` → clear to NULL
/// - `Some(Some(v))` = field present with value → set to new value
#[derive(Debug, Clone)]
pub enum Nullable<T> {
    Absent,
    Null,
    Value(T),
}

impl<T> Default for Nullable<T> {
    fn default() -> Self {
        Nullable::Absent
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Nullable<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // If this is called, the field was present in JSON
        Option::<T>::deserialize(deserializer).map(|opt| match opt {
            Some(v) => Nullable::Value(v),
            None => Nullable::Null,
        })
    }
}

impl<T> Nullable<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Nullable::Absent)
    }
}

// ── Board ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Board {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub board_type: String,
    pub sort_direction: String,
    pub tier_config: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    pub accumulative: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBoard {
    pub slug: String,
    pub name: String,
    pub board_type: String,
    #[serde(default = "default_sort_direction")]
    pub sort_direction: String,
    pub tier_config: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub accumulative: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBoard {
    pub name: Option<String>,
    pub sort_direction: Option<String>,
    #[serde(default)]
    pub tier_config: Nullable<serde_json::Value>,
    #[serde(default)]
    pub metadata: Nullable<serde_json::Value>,
}

fn default_sort_direction() -> String {
    "desc".to_string()
}

// ── Entry ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Entry {
    pub id: Uuid,
    pub board_id: Uuid,
    pub slug: String,
    pub name: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEntry {
    pub slug: String,
    pub name: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEntry {
    pub name: Option<String>,
    #[serde(default)]
    pub metadata: Nullable<serde_json::Value>,
}

// ── Version ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Version {
    pub id: Uuid,
    pub board_id: Uuid,
    pub version_number: i32,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Placement ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Placement {
    pub id: Uuid,
    pub version_id: Uuid,
    pub entry_id: Uuid,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlacement {
    pub entry_slug: String,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateVersion {
    pub note: Option<String>,
    pub placements: Vec<CreatePlacement>,
}

// ── Enriched responses ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct VersionWithPlacements {
    #[serde(flatten)]
    pub version: Version,
    pub placements: Vec<PlacementWithEntry>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PlacementWithEntry {
    pub id: Uuid,
    pub version_id: Uuid,
    pub entry_id: Uuid,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub entry_slug: String,
    pub entry_name: String,
}

#[derive(Debug, Serialize)]
pub struct BoardSummary {
    #[serde(flatten)]
    pub board: Board,
    pub latest_version: Option<i32>,
    pub entry_count: i64,
}

// ── History + Diffing ─────────────────────────────────────────────────────

/// A single entry's placement in one version (for history endpoint).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EntryHistoryItem {
    pub version_number: i32,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Full diff between two versions.
#[derive(Debug, Serialize)]
pub struct VersionDiff {
    pub from_version: i32,
    pub to_version: i32,
    pub added: Vec<DiffEntry>,
    pub removed: Vec<DiffEntry>,
    pub moved: Vec<DiffMovedEntry>,
    pub unchanged: Vec<DiffEntry>,
}

/// An entry that was added, removed, or unchanged between versions.
#[derive(Debug, Serialize)]
pub struct DiffEntry {
    pub entry_slug: String,
    pub entry_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

/// An entry that moved between versions (position, score, or tier changed).
#[derive(Debug, Serialize)]
pub struct DiffMovedEntry {
    pub entry_slug: String,
    pub entry_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_tier: Option<String>,
}

// ── References ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BoardReference {
    pub id: Uuid,
    pub board_id: Uuid,
    pub pinned_version_id: Option<Uuid>,
    pub uri: String,
    pub ref_type: String,
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReference {
    pub pinned_version_number: Option<i32>,
    pub uri: String,
    pub ref_type: String,
    pub label: Option<String>,
}
