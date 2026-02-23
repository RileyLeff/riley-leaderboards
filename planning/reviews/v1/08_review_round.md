# Review Round 2 — 2026-02-22

**Models**: Codex, Gemini, Claude
**Context**: ~63k tokens
**Phase**: Phase 2 Boards + Entries + Versions (exhaustive review, round 2)

## Fix Verification

All 6 round 1 fixes verified correct by all 3 models:
1. FOR UPDATE serialization for version numbers — correct
2. Nullable<T> PATCH semantics — correct
3. Slug validation — correct
4. Tiered ordering LATERAL join — correct
5. Entry deletion conflict check — correct
6. Ordered board duplicate position validation — correct

## Findings

### Major

**1. Entry deletion race condition — check + delete not transactional** [codex-only]
- **File**: `crates/riley-leaderboards-core/src/repo/entries.rs:87-112`
- **Description**: `entries::delete` checks `count(*) FROM placements` then does `DELETE FROM entries` as separate queries outside a transaction. A concurrent version creation could insert a placement between the count check (returns 0) and the delete, then `ON DELETE CASCADE` would cascade the new placement, silently mutating the version.
- **Fix**: Wrap the check + delete in a transaction. Use `SELECT ... FOR UPDATE` on the entry row to serialize against concurrent placement inserts.

**2. Mixed explicit/implicit position collisions in ordered boards** [consensus: Gemini + Claude]
- **File**: `crates/riley-leaderboards-core/src/repo/versions.rs`, `validate_placements` + `create`
- **Description**: `validate_placements` only checks uniqueness of explicitly provided positions. When some placements omit position (derived from array index), the implicit positions can collide with explicit ones. Example: `[{entry: "a"}, {entry: "b", position: 1}]` — "a" gets implicit position 1, "b" gets explicit position 1.
- Gemini: major, Claude: minor. Elevated to major — data integrity issue.
- **Fix**: Resolve all positions (filling in implicit from array order) before checking uniqueness.

### Minor

**3. tier_config shape not validated on create/update** [codex-only]
- **File**: `crates/riley-leaderboards-core/src/repo/boards.rs:13,98`
- **Description**: `tier_config` is accepted as arbitrary JSON. `fetch_placements` casts `(t.obj->>'position')::int` which will fail with a SQL error if the value is non-numeric. Malformed tier_config → 500 on version reads.
- **Fix**: Validate tier_config structure (must have `tiers` array, each with `key` string and `position` integer).

**4. Non-deterministic scored board tiebreaking** [gemini-only]
- **File**: `crates/riley-leaderboards-core/src/repo/versions.rs:201-210`
- **Description**: `ROW_NUMBER() OVER (ORDER BY score {order} NULLS LAST)` — when scores are equal, positions are non-deterministic. Different transactions could assign different positions for the same data.
- **Fix**: Add a tiebreaker: `ORDER BY score {order} NULLS LAST, id ASC`.

**5. No validation that name is non-empty** [claude-only]
- **File**: `crates/riley-leaderboards-core/src/repo/boards.rs`, `entries.rs`
- **Description**: Boards and entries can be created with `"name": ""`. DB only enforces NOT NULL.
- **Fix**: Add non-empty check for name in create/update for both boards and entries.

**6. Entry deletion error message imprecision** [claude-only]
- **File**: `crates/riley-leaderboards-core/src/repo/entries.rs:98-101`
- **Description**: Message says "version(s)" but counts placement rows. Due to UNIQUE(version_id, entry_id) these happen to equal, but semantically imprecise.
- **Fix**: Use `count(DISTINCT version_id)` or change wording to "placements".

### Notes

**7. Safety limits from plan not implemented** [consensus: Codex + Gemini]
- Plan specifies `max_entries_per_version = 1000`, `max_versions_per_board = 10000`, `max_metadata_size_bytes = 65536`. Not yet in config or enforced. Phase 8 (polish/operational) concern.

**8. N+1 queries in version creation** [consensus: all 3]
- Each placement requires 2 queries (slug lookup + insert). Acceptable at current scale, optimize in Phase 8 with batch queries.

**9. get_summary makes 3 sequential queries** [claude-only]
- Could be combined into 1 query with subqueries. Phase 8 optimization.

**10. No concurrency tests** [codex-only]
- Race conditions (version number, entry deletion) are not exercised by tests. Concurrency tests are complex but valuable for Phase 8.

**11. ON DELETE CASCADE remains on placements.entry_id** [claude-only]
- Application-level rejection prevents the cascade from firing for entry-level deletion. Schema CASCADE is appropriate for board-level deletion. Correct layered approach.

## Convergence

| Round | Major | Minor | Models |
|-------|-------|-------|--------|
| R1 | 2 | 5 | Claude only |
| R2 | 2 | 4 | Codex + Gemini + Claude |

**Not converged.** Need 0 major bugs in 2 consecutive rounds.
