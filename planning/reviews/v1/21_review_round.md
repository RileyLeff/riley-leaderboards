# Phase 5 Exhaustive Review — Round 2

**Date:** 2026-02-22
**Models:** Gemini + Claude
**Context:** ~90k tokens
**Scope:** Phase 5 (Accumulative Boards) convergence check

## Fix Verification

All 5 R1 fixes verified correct by both models.

## Findings

### Major
None.

### Minor

1. **No test for entry name update on re-submission** — The upsert test submitted the same name both times, so the `name = $3` fix had no test coverage. [Claude]
   - Fixed: commit fb016ea

2. **Snapshot uses inline placement fetch instead of shared `fetch_placements`** — Mild duplication. The inline query uses a simpler ORDER BY (no LATERAL join for tier ordering), which is functionally correct for accumulative boards (always scored, never tiered). [Claude]
   - Status: Accepted as note. The simpler query is appropriate for this context.

### Notes

- **Gemini false positive: "stale board config"** — Gemini flagged that `snapshot` uses the input `board` for validation before acquiring the lock. However, the code already re-fetches the board with `SELECT ... FOR UPDATE` (line 87), shadowing the input. The `sort_direction` used for position derivation comes from the locked board. The pre-lock checks are for `accumulative` and `board_type` which are immutable after creation. Not a bug.
- **Snapshot does not clear accumulated_scores** — By design. This is the "high score" pattern. A "reset on snapshot" flag would be a future enhancement. [Gemini]
- **No DB-level constraint on NaN/Infinity** — Application-level validation is correct and complete. DB constraint would be defense-in-depth. [Claude]
- **`f64` partial equality in diff** — Theoretical concern with NaN != NaN, but NaN is rejected at submission. [Claude]

## Convergence

| Round | Major | Minor | Models |
|-------|-------|-------|--------|
| R1    | 1     | 4     | Gemini + Claude |
| R2    | 0     | 2     | Gemini + Claude |

Not yet converged — need one more round with 0 major to reach 2 consecutive clean rounds.
