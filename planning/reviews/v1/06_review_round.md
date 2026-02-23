# Review Round 1 (Phase 2) — 2026-02-22

**Models**: Claude (Codex: no output file produced; Gemini: empty output)
**Context**: ~59k tokens
**Phase**: Phase 2 Boards + Entries + Versions (exhaustive review, round 1)

## Findings

### Major

1. **Race condition in version number assignment** — `versions.rs:19-24`
   `SELECT COALESCE(MAX(version_number), 0) + 1` is not serialized within the
   transaction. Concurrent version creation can produce duplicate numbers.
   Unique constraint prevents silent corruption but surfaces as HTTP 500.

2. **COALESCE prevents clearing optional fields via PATCH** — `boards.rs:79-95`,
   `entries.rs:62-74`
   `COALESCE($1, name)` treats JSON `null` and absent field identically. Once
   `metadata` or `tier_config` is set, there is no way to clear it to null.

### Minor

3. **No slug format validation** — `boards.rs`, `entries.rs`
   Slugs can contain URL-unsafe characters. Should validate lowercase
   alphanumeric + hyphens.

4. **No length limits on string inputs** — `models.rs`
   Plan specifies `max_metadata_size_bytes = 65536` and `max_entries_per_version
   = 1000` but nothing enforces these.

5. **Ordered board allows duplicate explicit positions** — `versions.rs:218-230`
   No uniqueness check on position values for ordered boards.

6. **Entry deletion cascades through placements** — `001_initial_schema.sql:44`
   `ON DELETE CASCADE` silently removes entry from all historical versions,
   contradicting "versions are immutable snapshots." Should be intentional.

7. **Tiered board response ordering ignores tier grouping** — `versions.rs:154-165`
   `ORDER BY position ASC` sorts globally, not tier-then-position.

### Notes

1. `board_type`/`accumulative` correctly immutable via PATCH
2. Nonexistent entry in version creation returns 400 (Validation) — intentional
3. Mixed explicit/implicit positions edge case — matches spec
4. PlacementWithEntry doesn't include entry metadata — may surface in frontend
5. No pagination — correctly deferred to Phase 8
6. `get_by_id` functions are dead code — harmless, may be useful later
7. Cross-board integrity enforced at app level — Phase 1 action item resolved
8. SQL injection surface is clean — all parameterized
9. Axum default rejections don't match API error shape — polish phase

## Convergence

| Round | Major | Minor | Models |
|-------|-------|-------|--------|
| 1     | 2     | 5     | Claude |

Not converged. Fix majors and re-review.
