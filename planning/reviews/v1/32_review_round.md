# Review Round 32 — Phase 7 Exhaustive R2

**Date:** 2026-02-22
**Models:** Claude (Codex timed out)
**Context:** ~120k tokens
**Scope:** Full codebase, all modules, after R1 fixes

## Verification of R1 Fixes

All five R1 fixes confirmed as correctly implemented:

1. Webhook git pull timeout (60s, 504 on expiry)
2. JWKS cache staleness (`last_refresh` + `JWKS_MAX_STALE_SECS`)
3. f64 bitwise comparison (`scores_equal` using `to_bits()`)
4. Case-insensitive Bearer prefix (`eq_ignore_ascii_case`)
5. Tiered board tier_config required at creation

## Major

None.

## Minor

1. **JWKS `last_refresh` updated even when refresh returns zero usable keys** — `auth.rs:161-162`. When the JWKS endpoint returns a response with no usable keys, `last_refresh` is still updated, making the cache appear "fresh." Should only update when `new_keys` is non-empty, so that a consistently empty JWKS response eventually triggers the fail-closed staleness path.

2. **`accumulative` field not validated at database level** — `migrations/001_initial_schema.sql:13`. The `accumulative` column has no CHECK constraint enforcing `accumulative = false OR board_type = 'scored'`. Application code enforces this, but a CHECK constraint would provide defense-in-depth.

## Notes

1. Auth middleware correctly scoped — webhook and health check outside `board_routes`
2. Narrow race window between entry deletion and version creation — safe (FK violation, retryable)
3. `validate_aud = false` is an explicit tradeoff for single-purpose deployments
4. Webhook runs synchronously — acceptable for v1 per prior review notes
5. `cors_origins` and `behind_proxy` config fields unused — Phase 8 items
6. `since` endpoint returns lightweight `Vec<Version>` without placements — intentional
7. Test coverage thorough — 81 integration tests
8. Schema isolation design sound — `quote_identifier` properly escapes
