# Phase 8 Exhaustive Review — Round 2

**Date**: 2026-02-23
**Models**: Claude Opus 4.6 (Gemini failed — shell syntax issue)
**Context**: ~134k tokens

## Verification of R1 Fixes

All four R1 major fixes confirmed as correctly implemented:

1. Import is now transactional (pool.begin / tx.commit)
2. Import pre-validates placements via validate_placements_for_import()
3. Import derives scored positions for scored boards
4. Webhook route conditional on sync config

All three R1 minor fixes also verified correct (last-seen entry name, CORS layer ordering, webhook git pull branch).

## Major

None.

## Minor

1. **Import does not validate board_type or sort_direction** — `export.rs` validates slug and name but not board_type/sort_direction. DB CHECK constraint catches invalid values, but error surfaces as raw DB error instead of clean validation error.

2. **derive_scored_positions tiebreaker non-determinism on import** — Entries get new UUIDv7 IDs on import (different timestamps), so scored position tiebreaker (`entry_id ASC`) may differ from original. Cosmetic inconsistency only; positions are always re-derived from scores.

3. **Webhook handler does not run migrations before sync** — CLI `sync` command runs migrations explicitly, but webhook assumes `serve` already migrated. Safe in practice (serve auto-migrates) but noted for robustness.

## Notes

1. `scores_equal()` duplicated in versions.rs and execute.rs — functionally correct, could be DRYed
2. Export doesn't include accumulated_scores or references — intentional design tradeoff
3. Import pre-validation Board with `Uuid::nil()` ID is safe (validate_placements never uses board.id)
4. Positive observations: auth scoping, transaction isolation, pagination, schema isolation, JWKS cache all correct
5. Test coverage thorough — 84 integration tests, no functional gaps

## Convergence

| Round | Major | Minor | Models |
|-------|-------|-------|--------|
| R1    | 4     | 13    | Claude only |
| R2    | 0     | 3     | Claude only |

**R1: 4 majors (all fixed). R2: 0 majors.** Need one more clean round for convergence.
