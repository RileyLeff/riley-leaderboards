# Phase 3 Exhaustive Review — Round 1

**Date:** 2026-02-22
**Models:** Gemini + Claude (Codex rate-limited)
**Context:** ~76k tokens
**Scope:** Full codebase, focus on Phase 3 (History + Diffing)

## Findings

### Major

None.

### Minor

1. **[claude-only] No validation on diff `from`/`to` parameters** — accepted negative, equal, and backwards values.
   - **Files:** `crates/riley-leaderboards-api/src/routes/versions.rs:52-71`
   - **Fix:** Added validation: `from >= 1`, `to >= 1`, `from < to`. Commit 075c312.

2. **[claude-only] `since` accepts negative version numbers** — `since/-5` returns all versions.
   - **Files:** `crates/riley-leaderboards-api/src/routes/versions.rs:73-80`
   - **Status:** Noted. Harmless — negative values just return all versions (same as `since/0`). Not worth adding validation for since it doesn't produce incorrect results.

3. **[claude-only] `HashMap<String, i32>` deserialization failures produce non-API-shaped errors** — non-integer query params trigger Axum's default 400.
   - **Files:** `crates/riley-leaderboards-api/src/routes/versions.rs:55`
   - **Status:** Noted. Axum framework behavior. Would require a custom extractor to fix. Low priority — clients sending non-integer version params are already violating the API contract.

4. **[gemini-only] Placement metadata changes not tracked in diff** — entries with only metadata changes classified as "unchanged".
   - **Files:** `crates/riley-leaderboards-core/src/repo/versions.rs:251-253`
   - **Status:** By design. The diff tracks ranking movement (position, score, tier), not metadata. Aligns with plan.md examples.

5. **[claude-only] Diff response shape differs slightly from plan examples** — plan shows `to_position` for added entries, implementation uses `position`.
   - **Files:** `crates/riley-leaderboards-core/src/models.rs:207-217`
   - **Status:** The implementation's naming is better — added entries only have one position. Plan examples will be updated in later phases.

### Notes

- **[consensus] Sequential queries in diff (4 queries)** — acceptable at v1 scale (~1000 entries), Phase 8 optimization target.
- **[gemini-only] Historical versions reflect current entry name** — by design per soul doc (entries are stable identity).
- **[gemini-only] `since` returns full Version objects** — includes internal `id` and `board_id`. Harmless, more complete than plan examples.
