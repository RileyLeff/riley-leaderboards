# Phase 2 Review Round 3 — Fixes

**Commit:** f3cec4f

## Minor Fixes

1. **Re-fetch board inside version creation tx** (finding #1)
   Changed `SELECT id FROM boards WHERE id = $1 FOR UPDATE` to
   `SELECT * FROM boards WHERE id = $1 FOR UPDATE` and use the re-fetched
   board for validation and all downstream logic. Pre-validation still runs
   with the initial snapshot for fast-fail, then re-validates after lock.

2. **tier_config position i32 range check** (finding #2)
   `validate_tier_config` now checks that `position` values fit in i32 range,
   matching the SQL `::int` cast in `fetch_placements`.

3. **Universal position >= 1 check** (finding #3)
   Added a pre-check loop across all board types that validates any explicit
   position is >= 1. The ordered board branch still handles uniqueness.

4. **Finite score validation** (finding #4)
   Scored board validation now rejects NaN and Infinity scores with a
   descriptive error message.

## Notes Recorded

- Lost update race in board/entry PATCH noted for Phase 8 / v2
- sort_direction on non-scored boards is harmless (Phase 8 documentation)
