# Fixes for Review Round 26 (Phase 6 R2)

**Date:** 2026-02-22

## Major Fixes

### 1. Ordered board implicit position reordering (Major #1)
- **File:** `core/src/sync/execute.rs` (`placements_changed`)
- **Fix:** Added `board_type` parameter to `placements_changed`. For ordered boards with implicit positions (`p.position.is_none()` and `board_type == "ordered"`), derive expected position from array index `(idx + 1) as i32` and compare against DB's stored position. This detects entry reordering in TOML files. For scored/tiered boards, position comparison is still skipped when position is None.

### 2. Ranking config forces new version (Major #2)
- **File:** `core/src/sync/execute.rs` (`sync_board`)
- **Fix:** Added `ranking_config_changed` tracking. When `sort_direction` or `tier_config` changed on an existing board, force `needs_version = true`. This ensures scored board positions are re-derived under the new sort direction.

## Minor Fixes

### 3. Parse JSON once in webhook (Minor #1)
- **File:** `api/src/routes/webhooks.rs` (`github`)
- **Fix:** Parse `serde_json::from_slice(&body)` once at the start of the handler. Extract `ref` and `head_commit.message` from the parsed value. Removed `extract_ref` and `extract_commit_message` helper functions.

### 4. Require X-GitHub-Event header (Minor #2)
- **File:** `api/src/routes/webhooks.rs` (`github`)
- **Fix:** Changed event type check to require the header (return 400 if missing) instead of falling through.

### 5. parse_boards_dir continues on errors (Minor #3)
- **File:** `core/src/sync/parse.rs` (`parse_boards_dir`)
- **Fix:** Catch per-board parse errors, log warning with `tracing::warn`, skip the failed board. Consistent with `sync_dir`'s per-board error handling.

### 6. HashMap for entry lookup (Minor #4)
- **File:** `core/src/sync/execute.rs` (`sync_board`)
- **Fix:** Replaced `existing_slugs` HashSet + linear `.find()` with `existing_entry_map` HashMap for O(1) entry lookups. Removed unused `existing_slugs`.

### 7. Warn on board_type mismatch (Minor #5)
- **File:** `core/src/sync/execute.rs` (`sync_board`)
- **Fix:** When existing board's `board_type` differs from TOML's `board_type`, log a warning explaining that board_type is immutable.

## All 73 tests passing.
