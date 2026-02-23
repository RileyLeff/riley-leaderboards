# Fixes for Review Round 30 — 2026-02-22

Commit: c35d696

## Major Fixes

1. **Webhook git pull timeout** — Added 60-second `tokio::time::timeout` wrapper around the `git pull` subprocess. Returns 504 Gateway Timeout on expiry. (webhooks.rs)

2. **JWKS cache fail-closed behavior** — Added `last_refresh` timestamp to `JwksCache`. `get_key()` now returns `Err` if the cache is stale beyond 2 hours (JWKS_MAX_STALE_SECS), preventing acceptance of tokens signed with potentially revoked keys. (auth.rs)

3. **f64 score comparison** — Replaced `!=` on `Option<f64>` with `scores_equal()` helper that uses `to_bits()` for bitwise comparison. Applied in both `placements_changed()` (execute.rs) and `version_diff()` (versions.rs).

## Minor Fixes

4. **Case-insensitive Bearer prefix** — Changed `extract_bearer_token` from `strip_prefix("Bearer ")` to case-insensitive ASCII comparison per RFC 7235 Section 2.1. (auth.rs)

5. **Tiered board without tier_config** — Changed `validate_tier_config` to reject `None` with a validation error instead of silently accepting it. Updated `tiered_board_requires_tier` test to verify the new behavior. (boards.rs, board_crud_test.rs)

## Deferred (Phase 8)

- No pagination on list endpoints — Phase 8 item
- `cors_origins` config not used — Phase 8 item (CORS middleware)
- `behind_proxy` config not used — Phase 8 item
- `since` endpoint inconsistency — design choice, will document
- `diff` endpoint memory usage — acceptable for v1 scale
- Webhook event type check order — negligible perf impact
- `board_type`/`accumulative` immutability by omission — design choice, documented in review notes
