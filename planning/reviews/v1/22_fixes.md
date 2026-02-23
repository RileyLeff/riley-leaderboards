# Phase 5 Review R2 — Fixes

**Commit:** fb016ea

## Fixes Applied

1. **Added test for entry name update on re-submission** — Modified `accumulative_score_upsert_behavior` to submit with a different name ("Player Updated") on re-submission and assert the snapshot reflects the updated name.
   - File: `crates/riley-leaderboards-api/tests/board_crud_test.rs`

## Accepted Notes (no fix needed)

- Inline placement fetch in snapshot: appropriate simpler query for scored-only context
- Gemini's "stale board config": false positive — board is re-fetched after lock
- No DB-level NaN/Infinity constraint: defense-in-depth consideration for future
- `f64` partial equality in diff: theoretical, NaN prevented at submission

## Test Results

All 62 tests passing (50 board_crud + 1 health + 7 unit + 4 db_integration).
