# Review Round 3 — 2026-02-22

**Models**: Gemini, Claude (Codex rate-limited)
**Context**: ~67k tokens
**Phase**: Phase 2 Boards + Entries + Versions (exhaustive review, round 3)

## Fix Verification

All 6 round 2 fixes verified correct by both Gemini and Claude:
1. Entry deletion race condition (transactional FOR UPDATE) — correct
2. Mixed explicit/implicit position collision detection — correct
3. tier_config shape validation — correct
4. Scored board tiebreaker (id ASC) — correct
5. Name non-empty validation — correct
6. Entry deletion error message (count DISTINCT version_id) — correct

## Findings

### Major

None.

Gemini flagged "lost update race in board/entry PATCH" as major (concurrent
PATCHes to different fields could overwrite each other). Downgraded to note:
this is a single-curator service per the soul doc, Phase 2 has no auth (one
user), and the window is within a single function call. Revisit in v2 if
multi-admin is added.

### Minor

**1. versions::create uses stale board snapshot** [consensus: Gemini + Claude]
- Pre-transaction board fetch could be outdated by time FOR UPDATE acquires.
- **Fix**: Re-fetch board inside tx after FOR UPDATE lock.

**2. tier_config validation uses i64 check for i32 SQL cast** [gemini-only]
- `as_i64()` check passes values > i32::MAX, but SQL `::int` would overflow.
- **Fix**: Validate position fits in i32 range.

**3. No position >= 1 check for scored/tiered boards** [claude-only]
- Ordered boards validated positions >= 1 but scored/tiered accepted 0 or negative.
- **Fix**: Add universal position >= 1 check for all board types.

**4. No NaN/Infinity check on scores** [claude-only]
- Currently protected by JSON deserialization, but defense-in-depth for future paths.
- **Fix**: Validate scores are finite.

### Notes

**5. Lost update race in board/entry PATCH** [gemini-only]
- Read-then-write without locking. Theoretical issue for concurrent multi-admin
  scenarios. Single-curator service, no practical risk in v1. Phase 8 or v2.

**6. sort_direction changeable on non-scored boards** [claude-only]
- Succeeds silently with no effect. Cosmetic confusion, not a bug. Document
  that sort_direction only affects scored boards.

**7. Codex rate-limited this round** — excluded from results.

## Convergence

| Round | Major | Minor | Models |
|-------|-------|-------|--------|
| R1 | 2 | 5 | Claude only |
| R2 | 2 | 4 | Codex + Gemini + Claude |
| R3 | 0 | 4 | Gemini + Claude (Codex rate-limited) |

**Consecutive rounds with zero major: 1.** Need 1 more for convergence.
