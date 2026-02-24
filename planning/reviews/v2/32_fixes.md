# Fixes for Review Round 31 (R2)

**Date:** 2026-02-23

## Minor Fixes

### Minor #1: Collections no-op guard moved to route handler
- **Files:** `crates/riley-leaderboards-api/src/routes/collections.rs`, `crates/riley-leaderboards-core/src/repo/collections.rs`
- Moved no-op PATCH guard from repo function to route handler, matching the boards update pattern. This avoids acquiring a FOR UPDATE lock for true no-ops.

### Minor #2: Health response now includes component status
- **File:** `crates/riley-leaderboards-api/src/lib.rs`
- Health response now returns `{"status": "ok", "postgres": "ok"}` and adds `"redis": "ok"` when Redis is configured and reachable.
- Also fixed clippy warning: `if let Err(_)` → `.is_err()` for Postgres check.

## Notes documented

- ConnectionGuard underflow: documented in review_notes_README.md
- JWKS cancellation style, export N+1, sort_direction message: accepted as-is

## Verification

- `cargo check --workspace`: clean
- `cargo test` (non-realtime): 128 tests pass, 0 failures
