# Phase 5 Review R1 — Fixes

**Commit:** a35ba1d

## Fixes Applied

1. **Non-deterministic tiebreaker (Major)** — Changed `id ASC` to `entry_id ASC` in `derive_scored_positions` window function. `entry_id` is stable across versions (same entry always gets the same UUID), while `placements.id` is a new uuidv7 per version insert.
   - File: `crates/riley-leaderboards-core/src/repo/versions.rs:219`

2. **Entry name not updated on re-submission** — Added `name = $3` to the `ON CONFLICT DO UPDATE SET` clause in `scores::submit`, so re-submitting with a new name updates the entry.
   - File: `crates/riley-leaderboards-core/src/repo/scores.rs`

3. **Accumulative + board_type validation** — Added check in `boards::create`: `accumulative = true` requires `board_type = "scored"`. Also added `board_type != "scored"` guard in `scores::snapshot` (belt + suspenders with the creation-time check).
   - Files: `crates/riley-leaderboards-core/src/repo/boards.rs`, `crates/riley-leaderboards-core/src/repo/scores.rs`

4. **Transaction wrapping** — Wrapped entry upsert + score upsert in `scores::submit` inside `pool.begin()` / `tx.commit()` for atomicity.
   - File: `crates/riley-leaderboards-core/src/repo/scores.rs`

5. **DRY position derivation** — Made `derive_scored_positions` public (`pub async fn`), reused it in `scores::snapshot` instead of duplicating the SQL. Removed inline position derivation from snapshot.
   - Files: `crates/riley-leaderboards-core/src/repo/versions.rs`, `crates/riley-leaderboards-core/src/repo/scores.rs`

6. **New test** — Added `accumulative_non_scored_board_rejected` test verifying that creating an accumulative ordered board returns 400.
   - File: `crates/riley-leaderboards-api/tests/board_crud_test.rs`

## Test Results

All 62 tests passing (50 board_crud + 1 health + 7 unit + 4 db_integration).
