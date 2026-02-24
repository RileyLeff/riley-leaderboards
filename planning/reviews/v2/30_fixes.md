# Fixes for Review Round 29

**Date:** 2026-02-23

## Major Fixes

### Major #2: Unbounded metric label cardinality — FIXED
- **File:** `crates/riley-leaderboards-api/src/metrics.rs`
- Changed: When `MatchedPath` is unavailable (404s, unmatched routes), the fallback is now `"unmatched"` instead of `request.uri().path()`. This prevents attackers from creating unbounded cardinality by hitting random paths.
- The `board` label on `scores_submitted_total` and `versions_created_total` is kept — board count is bounded and small in practice.

### Major #3: JWKS refresh task leak on shutdown — FIXED
- **Files:** `crates/riley-leaderboards-api/src/auth.rs`, `crates/riley-leaderboards-cli/src/main.rs`
- Changed: `spawn_refresh_task` now accepts `Option<&TaskTracker>` and uses `tracker.spawn()` when available, `tokio::spawn()` when not (tests).
- Deferred spawning: `from_config` no longer auto-spawns the refresh task. New `AuthMode::start_background_tasks(&self, tracker)` method is called from the CLI serve command after AppState construction. This ensures the task is tracked for graceful shutdown.

## Minor Fixes

### Minor #4: versions::since OpenAPI type mismatch — FIXED
- **File:** `crates/riley-leaderboards-api/src/routes/versions.rs`
- Changed: OpenAPI annotation updated from `Vec<VersionWithPlacements>` to `Vec<Version>` to match actual return type.

### Minor #8: boards::list OpenAPI type mismatch — FIXED
- **File:** `crates/riley-leaderboards-api/src/routes/boards.rs`
- Changed: OpenAPI annotation updated from `inline(PaginatedResponse<BoardSummary>)` to `inline(PaginatedResponse<Board>)` to match actual return type.

### Minor #10: collections::update missing FOR UPDATE — FIXED
- **File:** `crates/riley-leaderboards-core/src/repo/collections.rs`
- Changed: `update()` now uses a transaction with `SELECT * FROM collections WHERE slug = $1 FOR UPDATE` before the UPDATE, matching the pattern in boards/entries. Prevents TOCTOU on concurrent collection updates.

## Intentional / Deferred (not fixed)

### Major #1: Unauthenticated /metrics and /docs endpoints — INTENTIONAL
- See review_notes_README.md Phase 8 section.
- These endpoints are intentionally unauthenticated, protected at the reverse proxy layer.

### Minor #5-7, #9: Various OpenAPI annotation improvements — DEFERRED
- OpenAPI response type annotations for error responses (400, 404, 409) don't reference `ErrorResponse` schema. Cosmetic — the error shape is documented in the OpenAPI schema list.
- `LimitParam` IntoParams derive: would need utoipa IntoParams on a single-field struct. Low value.

## Verification

- `cargo check --workspace`: clean
- `cargo test --workspace` (non-realtime): 122 tests pass, 0 failures
- `cargo clippy --workspace`: no new warnings (pre-existing only)
