# Phase 2 Review Round 1 — Fixes

**Commit:** 1846d97

## Major Fixes

1. **Version number race condition** (finding #1)
   Added `SELECT id FROM boards WHERE id = $1 FOR UPDATE` before computing
   `MAX(version_number)` to serialize concurrent version creation. The board
   row lock ensures only one transaction at a time can compute the next number.

2. **COALESCE prevents clearing nullable fields** (finding #2)
   Introduced `Nullable<T>` enum with three states: `Absent` (field omitted,
   keep old), `Null` (explicit null, clear to NULL), `Value(T)` (set new value).
   Board and entry update DTOs use `Nullable<serde_json::Value>` for optional
   JSONB fields. SQL UPDATE now uses explicit `$1` params instead of COALESCE.

## Minor Fixes

3. **Slug format validation** (finding #3)
   Added `validate_slug()` to repo module. Slugs must be 1-128 chars, lowercase
   alphanumeric + hyphens, no leading/trailing hyphens. Applied to board and
   entry creation.

4. **Tiered board response ordering** (finding #18)
   `fetch_placements` now joins through versions→boards to access `tier_config`
   JSONB, using a LATERAL subquery to extract the tier's position value.
   Placements sort by tier position, then within-tier position, then name.

5. **Entry deletion cascade** (finding #16)
   Entry deletion now checks for existing placements. If any exist, returns
   409 Conflict with a message explaining the entry has placements. Prevents
   silent mutation of historical versions.

6. **Ordered board duplicate positions** (finding #7)
   Added position uniqueness check in `validate_placements` for ordered boards.
   Explicit positions must not collide (implicit positions from array order
   are inherently unique).

## New Tests

7 tests added covering:
- `invalid_board_slug_returns_400`
- `invalid_entry_slug_returns_400`
- `board_patch_can_clear_metadata_to_null`
- `board_patch_omitted_fields_keep_old_values`
- `entry_delete_with_placements_returns_409`
- `entry_delete_without_placements_succeeds`
- `ordered_board_duplicate_positions_returns_400`

## Notes Recorded

Review notes carried forward to review_notes_README.md:
- board_type/accumulative immutable via PATCH (intentional)
- Nonexistent entry in version creation returns 400 (Validation, not 404)
- Mixed explicit/implicit positions in ordered boards match spec
- PlacementWithEntry doesn't include entry metadata (future enhancement)
- No pagination (Phase 8)
- Axum default rejections don't match API error shape (polish phase)
