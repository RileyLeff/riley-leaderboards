# Fixes for Review Round 32 (R2)

**Commit:** f8af5d5

## Minor Fixes

1. **JWKS `last_refresh` only updated on non-empty key sets** — Moved `last_refresh` update inside the `!new_keys.is_empty()` branch. Empty JWKS responses no longer reset the staleness clock, so a consistently empty endpoint will eventually trigger the fail-closed "JWKS cache is stale" error instead of the confusing "unknown signing key" error.

2. **Accumulative CHECK constraint** — Added `migrations/002_accumulative_check.sql` with `CHECK (accumulative = false OR board_type = 'scored')`. Defense-in-depth matching the application-level validation in `boards.rs`.
