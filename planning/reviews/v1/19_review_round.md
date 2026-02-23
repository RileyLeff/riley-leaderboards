# Phase 5 Exhaustive Review — Round 1

**Date:** 2026-02-22
**Models:** Gemini + Claude (Codex rate-limited)
**Context:** ~88k tokens
**Scope:** Phase 5 (Accumulative Boards) — full codebase review

## Findings

### Major

1. **Non-deterministic tiebreaker in `derive_scored_positions`** — `ROW_NUMBER() OVER (ORDER BY score ... , id ASC)` uses `placements.id` (a new uuidv7 per version), causing position jitter for tied scores across snapshots. Should use `entry_id ASC` for stability. [Gemini]
   - File: `crates/riley-leaderboards-core/src/repo/versions.rs:219`

### Minor

2. **Entry name not updated on score re-submission** — `ON CONFLICT (board_id, slug) DO UPDATE SET updated_at = now()` ignores a changed `entry_name`. Should also set `name = $3`. [Gemini + Claude]
   - File: `crates/riley-leaderboards-core/src/repo/scores.rs` (`submit` function)

3. **`accumulative = true` not validated against `board_type`** — Can create accumulative ordered/tiered boards, which have no meaningful semantics. Should reject at creation time. [Gemini + Claude]
   - File: `crates/riley-leaderboards-core/src/repo/boards.rs` (`create` function)

4. **Score submission not wrapped in transaction** — Entry upsert and score upsert are two separate DB operations without atomicity guarantee. [Gemini]
   - File: `crates/riley-leaderboards-core/src/repo/scores.rs` (`submit` function)

5. **Snapshot duplicates position derivation logic** — `scores::snapshot` contains inline SQL for position derivation that duplicates `versions::derive_scored_positions`. Should reuse the shared function. [Gemini]
   - File: `crates/riley-leaderboards-core/src/repo/scores.rs` (`snapshot` function)

### Notes

- **Board locking in snapshot is correct** — `SELECT ... FOR UPDATE` serializes concurrent snapshots properly. [Gemini + Claude]
- **Accumulated score upsert is clean** — `ON CONFLICT (board_id, entry_id) DO UPDATE SET score = $3` correctly replaces previous score. [Claude]
- **Version guard on accumulative boards is well-placed** — Direct `POST /versions` returns clear error message directing users to `/snapshot`. [Claude]
- **Test coverage is thorough** — 8 tests covering CRUD lifecycle, upsert behavior, sort directions, error paths (non-accumulative, no scores, direct version create), cascading deletes, and multiple snapshots. [Claude]
