# Phase 2 Review Round 2 — Fixes

**Commit:** 4a9fb51

## Major Fixes

1. **Entry deletion race condition** (finding #1)
   Wrapped the placement check + delete in a transaction. Entry row is
   locked with `SELECT * FROM entries ... FOR UPDATE` before counting
   placements and deleting, serializing against concurrent version creation.
   Also fixed the error message to use `count(DISTINCT version_id)` for
   accurate version count reporting.

2. **Mixed explicit/implicit position collisions** (finding #2)
   `validate_placements` now resolves all positions — explicit from input,
   implicit from array order — before checking uniqueness. This catches
   collisions between `{entry: "a"}` (implicit pos 1) and
   `{entry: "b", position: 1}` (explicit pos 1).

## Minor Fixes

3. **tier_config shape validation** (finding #3)
   Added `validate_tier_config()` that checks tier_config has a `tiers`
   array, each element with a string `key` and integer `position`. Called
   on board create (tiered boards only) and board update (when tier_config
   is being set on a tiered board).

4. **Non-deterministic scored board tiebreaking** (finding #4)
   Added `id ASC` as a secondary sort key in the `ROW_NUMBER()` window
   function, ensuring consistent position assignment when scores are equal.

5. **Name validation** (finding #5)
   Added `validate_name()` to repo module. Names must be non-empty and
   ≤ 256 characters. Applied to board and entry create/update.

6. **Entry deletion error message** (finding #6)
   Changed from `count(*)` on placements to `count(DISTINCT version_id)`
   so the message accurately reports version count.

## New Tests

3 tests added covering:
- `ordered_board_mixed_explicit_implicit_position_collision`
- `tiered_board_invalid_tier_config_shape_returns_400`
- `empty_name_returns_400`

## Notes Recorded

Review notes carried forward:
- Safety limits (max_entries_per_version, etc.) are Phase 8 concern
- N+1 queries in version creation acceptable at current scale (Phase 8)
- get_summary sequential queries are Phase 8 optimization
- Concurrency tests are Phase 8
- ON DELETE CASCADE on placements.entry_id is correct layered approach
