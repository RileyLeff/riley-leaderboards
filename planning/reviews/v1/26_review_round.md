# Review Round 26 — Phase 6 Exhaustive R2

**Date:** 2026-02-22
**Models:** Gemini, Claude (Codex unavailable)
**Context:** ~107k tokens
**Scope:** Full codebase review, focus on Phase 6 (File Sync) after R1 fixes

## Findings

### Major

1. **Ordered board implicit position reordering not detected** [gemini]
   - File: `core/src/sync/execute.rs` (`placements_changed`)
   - When ordered board entries use implicit positions (no explicit `position` in TOML) and entries are reordered in the file, `placements_changed` skipped comparison because `p.position.is_none()`. The reorder went undetected, no new version created.
   - Fix: pass `board_type` to `placements_changed`. For ordered boards with `p.position.is_none()`, derive expected position from array index (1-based) and compare against DB's stored position.

2. **Ranking config changes don't force a new version** [gemini]
   - File: `core/src/sync/execute.rs` (`sync_board`)
   - If `sort_direction` or `tier_config` changed, the board metadata was updated but no new version was created. For scored boards, this meant the latest version's positions still reflected the old ordering.
   - Fix: track `ranking_config_changed` flag. When `sort_direction` or `tier_config` changed, force `needs_version = true`.

### Minor

1. **Duplicate JSON body parsing in webhook** [consensus]
   - File: `api/src/routes/webhooks.rs`
   - `extract_ref` and `extract_commit_message` both parsed the JSON body separately.
   - Fix: parse JSON once at the start of the handler, use the parsed value for ref and commit message extraction. Removed the two helper functions.

2. **Webhook event check proceeded if header was missing** [gemini]
   - File: `api/src/routes/webhooks.rs`
   - Missing `X-GitHub-Event` header fell through to sync instead of being rejected.
   - Fix: require the header, return 400 if missing.

3. **`parse_boards_dir` aborted on first parse error** [gemini]
   - File: `core/src/sync/parse.rs` (`parse_boards_dir`)
   - One malformed `board.toml` stopped all other boards from being parsed.
   - Fix: log warning and skip failed boards, consistent with `sync_dir` error handling.

4. **O(N^2) entry lookup in sync** [consensus]
   - File: `core/src/sync/execute.rs` (`sync_board`, entry update loop)
   - Linear search with `.find()` inside the entry loop.
   - Fix: replaced with `HashMap<&str, &Entry>` for O(1) lookups.

5. **No warning when TOML board_type differs from DB** [claude]
   - File: `core/src/sync/execute.rs` (`sync_board`)
   - `board_type` is immutable but if TOML changed it, the change was silently dropped.
   - Fix: log a warning when the values differ.

### Notes

1. **Sync bypasses API layer** — Both reviewers re-flagged. Already documented in review_notes_README.md as intentional v1 deviation. Not a new finding.

2. **Webhook runs synchronously** [claude] — May exceed GitHub's 10-second delivery timeout for large repos. Acceptable for v1. Background task approach would be a Phase 8 enhancement.

3. **No atomicity in sync_board** [gemini second pass] — Board/entry updates committed independently from version creation. Acceptable tradeoff for v1 simplicity.

4. **Concurrent webhooks safe due to FOR UPDATE locking** [claude] — Database integrity preserved, worst case is duplicate NoChange results.

5. **Test coverage solid** [claude] — All board types, sync scenarios, and webhook paths covered.
