use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// ── Nullable (for PATCH semantics) ─────────────────────────────────────────

/// Three-state type for PATCH operations:
/// - `None` = field absent from JSON → keep existing value
/// - `Some(None)` = field present as `null` → clear to NULL
/// - `Some(Some(v))` = field present with value → set to new value
#[derive(Debug, Clone, Default)]
pub enum Nullable<T> {
    #[default]
    Absent,
    Null,
    Value(T),
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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct Board {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub board_type: String,
    pub sort_direction: String,
    #[schema(value_type = Option<serde_json::Value>)]
    pub tier_config: Option<serde_json::Value>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
    pub accumulative: bool,
    pub realtime: bool,
    pub clear_on_snapshot: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateBoard {
    pub slug: String,
    pub name: String,
    pub board_type: String,
    #[serde(default = "default_sort_direction")]
    #[schema(default = "desc")]
    pub sort_direction: String,
    #[schema(value_type = Option<serde_json::Value>)]
    pub tier_config: Option<serde_json::Value>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub accumulative: bool,
    #[serde(default)]
    pub realtime: bool,
    #[serde(default)]
    pub clear_on_snapshot: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateBoard {
    pub name: Option<String>,
    pub sort_direction: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<Option<serde_json::Value>>)]
    pub tier_config: Nullable<serde_json::Value>,
    #[serde(default)]
    #[schema(value_type = Option<Option<serde_json::Value>>)]
    pub metadata: Nullable<serde_json::Value>,
    pub realtime: Option<bool>,
    pub clear_on_snapshot: Option<bool>,
}

fn default_sort_direction() -> String {
    "desc".to_string()
}

// ── Entry ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct Entry {
    pub id: Uuid,
    pub board_id: Uuid,
    pub slug: String,
    pub name: String,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateEntry {
    pub slug: String,
    pub name: String,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateEntry {
    pub name: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<Option<serde_json::Value>>)]
    pub metadata: Nullable<serde_json::Value>,
}

// ── Version ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct Version {
    pub id: Uuid,
    pub board_id: Uuid,
    pub version_number: i32,
    pub note: Option<String>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

// ── Placement ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct Placement {
    pub id: Uuid,
    pub version_id: Uuid,
    pub entry_id: Uuid,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePlacement {
    pub entry_slug: String,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateVersion {
    pub note: Option<String>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
    pub placements: Vec<CreatePlacement>,
}

// ── Enriched responses ─────────────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
pub struct VersionWithPlacements {
    #[serde(flatten)]
    pub version: Version,
    pub placements: Vec<PlacementWithEntry>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct PlacementWithEntry {
    pub id: Uuid,
    pub version_id: Uuid,
    pub entry_id: Uuid,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
    pub entry_slug: String,
    pub entry_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BoardSummary {
    #[serde(flatten)]
    pub board: Board,
    pub latest_version: Option<i32>,
    pub entry_count: i64,
}

// ── History + Diffing ─────────────────────────────────────────────────────

/// A single entry's placement in one version (for history endpoint).
#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct EntryHistoryItem {
    pub version_number: i32,
    pub position: Option<i32>,
    pub score: Option<f64>,
    pub tier: Option<String>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Full diff between two versions.
#[derive(Debug, Serialize, ToSchema)]
pub struct VersionDiff {
    pub from_version: i32,
    pub to_version: i32,
    pub added: Vec<DiffEntry>,
    pub removed: Vec<DiffEntry>,
    pub moved: Vec<DiffMovedEntry>,
    pub unchanged: Vec<DiffEntry>,
}

/// An entry that was added, removed, or unchanged between versions.
#[derive(Debug, Serialize, ToSchema)]
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
#[derive(Debug, Serialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct BoardReference {
    pub id: Uuid,
    pub board_id: Uuid,
    pub pinned_version_id: Option<Uuid>,
    pub pinned_version_number: Option<i32>,
    pub uri: String,
    pub ref_type: String,
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateReference {
    pub pinned_version_number: Option<i32>,
    pub uri: String,
    pub ref_type: String,
    pub label: Option<String>,
}

// ── Collections ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct Collection {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCollection {
    pub slug: String,
    pub name: String,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateCollection {
    pub name: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<Option<serde_json::Value>>)]
    pub metadata: Nullable<serde_json::Value>,
}

/// A board within a collection, with its display order and summary info.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct CollectionBoardEntry {
    pub slug: String,
    pub name: String,
    pub board_type: String,
    pub display_order: i32,
    pub latest_version: Option<i32>,
}

/// Response for GET /collections/:slug — collection metadata + its boards.
#[derive(Debug, Serialize, ToSchema)]
pub struct CollectionWithBoards {
    #[serde(flatten)]
    pub collection: Collection,
    pub boards: Vec<CollectionBoardEntry>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddBoardToCollection {
    pub board_slug: String,
    #[serde(default)]
    pub display_order: i32,
}

// ── Accumulated Scores ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct AccumulatedScore {
    pub id: Uuid,
    pub board_id: Uuid,
    pub entry_id: Uuid,
    pub score: f64,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SubmitScore {
    pub entry_slug: String,
    pub entry_name: String,
    pub score: f64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SnapshotInput {
    pub note: Option<String>,
    #[schema(value_type = Option<serde_json::Value>)]
    pub metadata: Option<serde_json::Value>,
}

// ── Pagination ──────────────────────────────────────────────────────────

pub const DEFAULT_PAGE_LIMIT: i64 = 50;
pub const MAX_PAGE_LIMIT: i64 = 200;

/// Simple limit-only parameter for endpoints that don't need cursor pagination.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct LimitParam {
    pub limit: Option<i64>,
}

impl LimitParam {
    pub fn effective_limit(&self) -> i64 {
        self.limit
            .unwrap_or(DEFAULT_PAGE_LIMIT)
            .clamp(1, MAX_PAGE_LIMIT)
    }
}

/// Cursor-based pagination parameters.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PaginationParams {
    /// Maximum number of items to return.
    pub limit: Option<i64>,
    /// Opaque cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
}

impl PaginationParams {
    pub fn effective_limit(&self) -> i64 {
        self.limit
            .unwrap_or(DEFAULT_PAGE_LIMIT)
            .clamp(1, MAX_PAGE_LIMIT)
    }

    /// Decode the cursor into (created_at, id). Returns None if no cursor.
    pub fn decode_cursor(&self) -> crate::error::Result<Option<(DateTime<Utc>, Uuid)>> {
        let Some(cursor) = &self.cursor else {
            return Ok(None);
        };
        let parts: Vec<&str> = cursor.splitn(2, ',').collect();
        if parts.len() != 2 {
            return Err(crate::error::Error::Validation(
                "invalid cursor format".to_string(),
            ));
        }
        let ts: DateTime<Utc> = parts[0]
            .parse()
            .map_err(|_| crate::error::Error::Validation("invalid cursor timestamp".to_string()))?;
        let id: Uuid = parts[1]
            .parse()
            .map_err(|_| crate::error::Error::Validation("invalid cursor id".to_string()))?;
        Ok(Some((ts, id)))
    }
}

/// Paginated response wrapper.
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T: Serialize + ToSchema> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Encode a cursor from created_at + id.
pub fn encode_cursor(created_at: &DateTime<Utc>, id: &Uuid) -> String {
    format!("{},{}", created_at.to_rfc3339(), id)
}
