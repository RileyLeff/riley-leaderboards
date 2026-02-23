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
