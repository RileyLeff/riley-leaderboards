//! OpenAPI specification and Swagger UI integration.

use utoipa::OpenApi;

/// Error response body shape used across all endpoints.
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Riley Leaderboards API",
        version = "2.0",
        description = "A general-purpose versioned ranking service.\n\n\
            **Authentication:** Write operations (POST, PATCH, DELETE) always require a Bearer token. \
            Read operations (GET) are public by default, but can be restricted via `require_read_auth` in server config."
    ),
    paths(
        // Boards
        crate::routes::boards::list,
        crate::routes::boards::create,
        crate::routes::boards::get,
        crate::routes::boards::update,
        crate::routes::boards::delete,
        // Entries
        crate::routes::entries::list,
        crate::routes::entries::create,
        crate::routes::entries::get,
        crate::routes::entries::update,
        crate::routes::entries::delete,
        crate::routes::entries::history,
        // Versions
        crate::routes::versions::list,
        crate::routes::versions::create,
        crate::routes::versions::get,
        crate::routes::versions::latest,
        crate::routes::versions::diff,
        crate::routes::versions::since,
        // Scores
        crate::routes::scores::submit,
        crate::routes::scores::snapshot,
        // References
        crate::routes::references::list,
        crate::routes::references::create,
        crate::routes::references::delete,
        // Collections
        crate::routes::collections::list,
        crate::routes::collections::create,
        crate::routes::collections::get,
        crate::routes::collections::update,
        crate::routes::collections::delete,
        crate::routes::collections::add_board,
        crate::routes::collections::remove_board,
    ),
    components(schemas(
        riley_leaderboards_core::models::Board,
        riley_leaderboards_core::models::CreateBoard,
        riley_leaderboards_core::models::UpdateBoard,
        riley_leaderboards_core::models::Entry,
        riley_leaderboards_core::models::CreateEntry,
        riley_leaderboards_core::models::UpdateEntry,
        riley_leaderboards_core::models::Version,
        riley_leaderboards_core::models::Placement,
        riley_leaderboards_core::models::CreatePlacement,
        riley_leaderboards_core::models::CreateVersion,
        riley_leaderboards_core::models::VersionWithPlacements,
        riley_leaderboards_core::models::PlacementWithEntry,
        riley_leaderboards_core::models::BoardSummary,
        riley_leaderboards_core::models::EntryHistoryItem,
        riley_leaderboards_core::models::VersionDiff,
        riley_leaderboards_core::models::DiffEntry,
        riley_leaderboards_core::models::DiffMovedEntry,
        riley_leaderboards_core::models::BoardReference,
        riley_leaderboards_core::models::CreateReference,
        riley_leaderboards_core::models::Collection,
        riley_leaderboards_core::models::CreateCollection,
        riley_leaderboards_core::models::UpdateCollection,
        riley_leaderboards_core::models::CollectionBoardEntry,
        riley_leaderboards_core::models::CollectionWithBoards,
        riley_leaderboards_core::models::AddBoardToCollection,
        riley_leaderboards_core::models::SubmitScore,
        riley_leaderboards_core::models::SnapshotInput,
        riley_leaderboards_core::models::AccumulatedScore,
        ErrorResponse,
    )),
    security(
        ("bearer_auth" = [])
    ),
    modifiers(&SecurityAddon),
)]
pub struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_auth",
            utoipa::openapi::security::SecurityScheme::Http(
                utoipa::openapi::security::HttpBuilder::new()
                    .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}
