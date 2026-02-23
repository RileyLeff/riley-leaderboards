# Phase 9 Exhaustive Review -- Round 2

**Date**: 2026-02-23
**Model**: Claude Opus 4.6
**Context**: ~141k tokens

## Verification of R1 Fixes

### Fix 1: Caddy path prefix stripping -- CONFIRMED CORRECT

**File:** `Caddyfile.snippet`

The directive has been changed from `handle` to `handle_path`:

```
handle_path /api/leaderboards/* {
    reverse_proxy leaderboards:8082
}
```

`handle_path` automatically strips the matched prefix before forwarding to upstream. Requests to `/api/leaderboards/boards` will arrive at the service as `/boards`, matching the router's registered routes. This fix is correct and complete.

### Fix 2: Webhook concurrency protection -- CONFIRMED CORRECT

**Files:** `crates/riley-leaderboards-api/src/lib.rs` (AppState), `crates/riley-leaderboards-api/src/routes/webhooks.rs`, `crates/riley-leaderboards-cli/src/main.rs`

The fix adds `sync_mutex: tokio::sync::Mutex<()>` to `AppState` (line 4021). The webhook handler acquires this mutex at line 4662 (`let _sync_guard = state.sync_mutex.lock().await;`) before starting `git pull` + `sync_dir`. The guard is held through both the git operation and the sync, ensuring serialization.

The mutex is correctly initialized in all construction sites:
- `main.rs` serve command (line 8879): `sync_mutex: tokio::sync::Mutex::new(())`
- `board_crud_test.rs` setup (line 4801): `sync_mutex: tokio::sync::Mutex::new(())`
- `health_test.rs` (line 8743): `sync_mutex: tokio::sync::Mutex::new(())`

The mutex placement is correct -- it is acquired after validation (HMAC, branch filter, event type, JSON parse) but before any filesystem or database side effects. This means invalid requests don't contend on the lock.

No issues with this fix.

## Major

None.

Both R1 major fixes are correctly implemented. No new major issues introduced. No regressions detected in previously fixed areas (JWKS staleness fail-closed, webhook HMAC verification, branch filtering, git pull timeout, import transactionality, scored-position derivation, schema isolation, entry deletion protection).

## Minor

1. **[carried from R1 #1] `behind_proxy` config field parsed but never used**
   - **File:** `crates/riley-leaderboards-core/src/config.rs` (line 9070), `crates/riley-leaderboards-api/src/lib.rs` (`build_router`)
   - `ServerConfig.behind_proxy` is deserialized but never referenced in the router builder. Behind Caddy, `tower_governor` will key rate limiting by Caddy's IP (single IP for all clients), effectively rate limiting all users collectively.

2. **[carried from R1 #3] JWKS cache wipes all keys when refresh returns zero usable keys**
   - **File:** `crates/riley-leaderboards-api/src/auth.rs` (lines 3774-3782)
   - When `new_keys` is empty, the code correctly skips updating `last_refresh` but still executes `*self.keys.write().await = new_keys;`, replacing the valid key map with an empty one. A transient JWKS endpoint issue returning an empty keyset wipes cached keys immediately. The staleness check at `get_key` would eventually fail closed (after 2 hours), but during the interval between wipe and staleness timeout, all JWT validations fail with "unknown signing key" rather than the more accurate "JWKS cache is stale" error.

3. **[carried from R1 #4] Tiered-board tier_config can be nulled via PATCH**
   - **File:** `crates/riley-leaderboards-core/src/repo/boards.rs` (`update`, line 9889-9893)
   - `validate_tier_config` is only called for `Nullable::Value`; `Nullable::Null` is accepted, allowing a tiered board to have `tier_config = NULL`. Subsequent version creation would still work (tier validation falls back to "no valid tiers" meaning any tier is accepted), but it contradicts the creation invariant that tiered boards must have tier_config.

4. **[carried from R1 #5] No `restart` policy in deploy compose fragment**
   - **File:** `docker-compose.deploy.yml`
   - The leaderboards service has no restart policy. A crash in production requires manual intervention.

5. **[carried from R1 #6] No auth integration tests**
   - **Files:** `tests/integration/run.sh`, `tests/integration/config.toml`
   - All integration tests run with no auth configured. Auth-protected write endpoints are never tested with authentication in the Docker smoke test suite.

6. **[carried from R1 #7] `diff` endpoint uses `HashMap<String, i32>` for query params**
   - **File:** `crates/riley-leaderboards-api/src/routes/versions.rs` (line 4470)
   - Non-integer values for `from` or `to` silently fail deserialization into the HashMap, producing "missing parameter" errors instead of "invalid integer" errors. A typed struct (`DiffParams { from: i32, to: i32 }`) would give clearer error messages.

7. **[carried from R1 #8] No healthcheck in deploy compose**
   - **File:** `docker-compose.deploy.yml`
   - The leaderboards service has a `/health` endpoint but the deploy compose doesn't define a healthcheck directive.

8. **[carried from R1 #9] CASCADE FK on placements.entry_id contradicts version immutability intent**
   - **File:** `migrations/001_initial_schema.sql` (line 12194)
   - `ON DELETE CASCADE` on `placements.entry_id` means a direct SQL `DELETE FROM entries` would silently remove historical placements. The application blocks this (returns 409), but `ON DELETE RESTRICT` would provide defense-in-depth at the schema level.

9. **[carried from R1 #10] Safety limits from plan not implemented**
   - **Files:** `planning/v1/plan.md`, `crates/riley-leaderboards-core/src/config.rs`
   - `max_entries_per_version`, `max_versions_per_board`, `max_metadata_size_bytes` are specified in the plan but not enforced. No protection against unbounded data creation beyond database storage limits.

10. **[carried from R1 #11] Import surfaces raw DB errors for invalid board_type/sort_direction**
    - **File:** `crates/riley-leaderboards-core/src/repo/export.rs` (`import_board`)
    - Import pre-validates placements, slug, and name, but `board_type` and `sort_direction` are not validated before the INSERT. Invalid enum values fail at DB CHECK constraint time with an opaque database error rather than a clear validation error.

11. **[carried from R1 #12] Dockerfile lacks dependency layer caching**
    - **File:** `Dockerfile`
    - The `COPY crates/ crates/` step invalidates the dependency-download layer on every source change. A stub-build pattern (copy Cargo.toml/lock first, build deps, then copy source) would improve rebuild times.

12. **[carried from R1 #2] Integration tests don't exercise deployment path**
    - **Files:** `tests/integration/run.sh`, `tests/integration/docker-compose.test.yml`
    - Tests hit the service directly on `:18082`, bypassing Caddy routing and the baked config at `/etc/riley_leaderboards/config.toml`. Deployment-path regressions (like the R1 Caddy issue) pass CI undetected.

## Notes

1. **No regressions detected.** All previously verified areas remain correct: JWKS staleness fail-closed logic, webhook HMAC-SHA256 verification with constant-time comparison, branch filtering, git pull timeout (60s), import transactionality, scored-position derivation with window functions, schema isolation with `search_path`, entry deletion protection via FOR UPDATE + conflict check.

2. **`scores_equal` function duplicated** in `versions.rs` (line 11384) and `execute.rs` (line 11752). Both use identical `to_bits()` comparison. Not a bug, but a minor code smell.

3. **Webhook handler correctly placed outside auth middleware.** The webhook route is registered directly on the app (line 4038-4042), not nested under `board_routes` which has the auth middleware layer. This means webhook requests are authenticated via HMAC-SHA256 only, not JWT/API token -- which is correct for GitHub webhooks.

4. **Rate limiter `expect()` on invalid config.** Line 4054 uses `.expect("invalid rate limit config")` which will panic at startup if the rate limit configuration is nonsensical. This is acceptable -- startup panics for invalid config are standard practice.

5. **Test coverage remains strong.** All test files correctly initialize `sync_mutex` in their `AppState` construction, confirming no compilation breakage from the R1 fix.

6. **Graceful shutdown handles SIGINT + SIGTERM** (lines 4189-4200), correct for Docker deployments.

7. **CORS layer correctly positioned as outermost layer** (line 4073), ensuring OPTIONS preflight requests pass through before rate limiting.

8. **Cursor-based pagination correctly implemented** across boards, entries, versions, and references, using `(created_at, id)` composite cursors with consistent ordering.

## Convergence

| Round | Major | Minor | Models |
|-------|-------|-------|--------|
| R1    | 2     | 12    | Codex + Claude |
| R2    | 0     | 12 (all carried from R1) | Claude only |

**R2: 0 majors. Both R1 fixes verified correct. No new issues introduced. No regressions. Converged.**
