# Review Round 4 — 2026-02-22

**Models**: Gemini, Claude (Codex rate-limited)
**Context**: ~69k tokens
**Phase**: Phase 2 Boards + Entries + Versions (exhaustive review, round 4 — convergence check)

## Fix Verification

All 4 round 3 fixes verified correct by both models:
1. Re-fetch board inside version creation tx (FOR UPDATE + re-validate) — correct
2. tier_config position i32 range check — correct
3. Universal position >= 1 check — correct
4. Finite score validation (NaN/Infinity) — correct

## Findings

### Major

None.

Gemini flagged a race condition between `versions::create` (plain SELECT on
entry) and `entries::delete` (FOR UPDATE on entry), claiming a placement could
be inserted and then CASCADE-deleted. **This is a false positive.**

PostgreSQL's FK constraint enforcement on `INSERT INTO placements` implicitly
acquires `FOR KEY SHARE` on the referenced entry row. `FOR KEY SHARE` conflicts
with `FOR UPDATE`. So:
- If `entries::delete` holds FOR UPDATE first: the placement INSERT blocks
  until delete commits (FK violation) or rolls back (INSERT proceeds).
- If `versions::create` inserts first: the KEY SHARE blocks entries::delete's
  FOR UPDATE, and when the version commits, the delete's placement count check
  will see the new placement and return 409.

Either way, version immutability is preserved.

### Minor

**1. tier_config allows duplicate keys** [gemini-only]
- `validate_tier_config` doesn't check key uniqueness. Duplicate keys would
  produce non-deterministic LATERAL join results. Note for Phase 8.

**2. Magic number 2147483647 in fetch_placements** [gemini-only]
- `COALESCE(tc.tier_ord, 2147483647)` could use `NULLS LAST` instead. Cosmetic.
  Note for Phase 8.

### Notes

Claude found 0 major, 0 minor, 7 notes — all performance optimizations and
previously-acknowledged items deferred to Phase 8. Code quality praised across
all dimensions.

## Convergence

| Round | Major | Minor | Models |
|-------|-------|-------|--------|
| R1 | 2 | 5 | Claude only |
| R2 | 2 | 4 | Codex + Gemini + Claude |
| R3 | 0 | 4 | Gemini + Claude |
| R4 | 0 | 2 | Gemini + Claude |

**2 consecutive rounds with zero major bugs. Exhaustive review CONVERGED.**

Phase 2 (Boards + Entries + Versions) is complete and ready for Phase 3.
