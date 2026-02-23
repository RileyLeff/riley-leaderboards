# Phase 9 Exhaustive Review -- Round 1

**Date**: 2026-02-23
**Models**: Codex, Claude (Gemini failed -- shell syntax)
**Context**: ~138k tokens

## Major

1. **[consensus] Caddy path prefix not stripped -- all proxied API traffic will 404 in production**
   - **Files:** `Caddyfile.snippet`, `crates/riley-leaderboards-api/src/lib.rs` (`build_router`)
   - **Details:** The Caddyfile uses `handle /api/leaderboards/* { reverse_proxy leaderboards:8082 }`, which forwards the full path (e.g., `/api/leaderboards/boards`) to upstream. The API router registers routes at `/health`, `/boards`, etc. -- not under `/api/leaderboards/`. Without stripping the prefix, every proxied request returns 404.
   - **Why invisible in CI:** Integration tests hit the service directly on `:18082`, bypassing Caddy.
   - **Fix:** Use `handle_path` (strips automatically) or add `uri strip_prefix /api/leaderboards`:
     ```
     handle_path /api/leaderboards/* {
         reverse_proxy leaderboards:8082
     }
     ```

2. **[claude-only] Webhook handler runs git pull + sync without concurrency protection**
   - **Files:** `crates/riley-leaderboards-api/src/routes/webhooks.rs`
   - **Details:** If two push events arrive in quick succession (GitHub retry, force-push + push), two concurrent `git pull` + `sync_dir` invocations race on the same filesystem directory. This can corrupt git state or create interleaved sync operations with partial results. The `FOR UPDATE` lock on board rows serializes individual version creation, but the git pull itself and the multi-board sync-dir operation are unprotected.
   - **Fix:** Add a `tokio::sync::Mutex` (or `Semaphore(1)`) to `AppState` that the webhook handler acquires before starting pull+sync.

## Minor

1. **[consensus] `behind_proxy` config field is parsed but never used -- rate limiting broken behind reverse proxy**
   - **Files:** `crates/riley-leaderboards-core/src/config.rs` (`ServerConfig.behind_proxy`), `crates/riley-leaderboards-api/src/lib.rs` (`build_router`)
   - **Details:** `behind_proxy` is defined in config and the example TOML but never read. `tower_governor` keys by direct connection IP, so behind Caddy all requests appear from one IP, causing collective rate limiting.
   - **Fix:** Implement proxy-aware key extraction when `behind_proxy = true`, or remove the field.

2. **[consensus] Integration tests don't exercise the actual deployment path (Caddy ingress + baked config)**
   - **Files:** `tests/integration/run.sh`, `tests/integration/docker-compose.test.yml`
   - **Details:** Tests hit the service directly on `:18082` and override config via `RILEY_LEADERBOARDS_CONFIG`. Neither the Caddy routing nor the baked `/etc/riley_leaderboards/config.toml` is tested. Deployment-path regressions (like the Caddy issue) pass CI undetected.

3. **[claude-only] JWKS cache wipes all keys when refresh returns zero usable keys**
   - **Files:** `crates/riley-leaderboards-api/src/auth.rs`
   - **Details:** When `refresh()` returns zero usable keys, the code correctly skips updating `last_refresh` (preserving staleness detection), but still replaces the key map with an empty map. A transient JWKS endpoint issue that returns an empty keyset will wipe all cached valid keys, causing immediate JWT validation failures until the next successful refresh.
   - **Fix:** Only update `self.keys` when `new_keys` is non-empty.

4. **[codex-only] Tiered-board tier_config can be nulled via PATCH, bypassing creation invariant**
   - **Files:** `crates/riley-leaderboards-core/src/repo/boards.rs` (`update`)
   - **Details:** `validate_tier_config` is only called for `Nullable::Value`; `Nullable::Null` is accepted and persisted, allowing a tiered board to end up with `tier_config = NULL` even though creation requires it.

5. **[claude-only] No `restart` policy in deploy compose fragment**
   - **Files:** `docker-compose.deploy.yml`
   - **Details:** If the service crashes in production, it won't restart automatically. Add `restart: unless-stopped`.

6. **[claude-only] Integration tests don't verify auth-protected endpoints**
   - **Files:** `tests/integration/run.sh`, `tests/integration/config.toml`
   - **Details:** All 14 tests run with no auth. There's no validation that auth actually blocks unauthenticated writes or that the Docker image works with auth enabled.

7. **[claude-only] `diff` endpoint uses `HashMap<String, i32>` -- poor error messages for invalid params**
   - **Files:** `crates/riley-leaderboards-api/src/routes/versions.rs`
   - **Details:** Using a HashMap means non-integer query values silently fail to appear in the map, giving "missing parameter" errors instead of "invalid integer" errors. A typed struct would give better validation.

8. **[claude-only] No healthcheck in deploy compose for leaderboards service**
   - **Files:** `docker-compose.deploy.yml`
   - **Details:** The service has a `/health` endpoint but the deploy compose doesn't define a healthcheck. Other services depending on leaderboards would have no readiness signal.

9. **[claude-only] CASCADE FK on placements from entries contradicts version immutability**
   - **Files:** `migrations/001_initial_schema.sql`
   - **Details:** `ON DELETE CASCADE` on `placements.entry_id` means direct SQL entry deletion would silently remove historical placements. The application layer correctly blocks this (returns 409), but `ON DELETE RESTRICT` would provide defense-in-depth.

10. **[claude-only] Safety limits from plan.md not implemented**
    - **Files:** `planning/v1/plan.md`, `crates/riley-leaderboards-core/src/config.rs`
    - **Details:** `max_entries_per_version`, `max_versions_per_board`, `max_metadata_size_bytes` are specified in the plan but not implemented. No protection against unbounded data creation.

11. **[consensus] Import still surfaces raw DB errors for invalid board enum fields (carried from Phase 8)**
    - **Files:** `crates/riley-leaderboards-core/src/repo/export.rs` (`import_board`)
    - **Details:** Import pre-validates placements/slug/name but not `board_type`/`sort_direction`; invalid values fail at DB CHECK constraint time.

12. **[claude-only] Dockerfile lacks layer caching for dependencies**
    - **Files:** `Dockerfile`
    - **Details:** Source changes invalidate the dependency-download layer. A stub-build pattern would improve rebuild times.

## Notes

1. **No regressions found in previously fixed areas.** JWKS staleness fail-closed, webhook hardening (HMAC, branch filter, timeout), import transactionality, and scored-position derivation all remain intact.
2. **Architecture aligned with soul/plan.** Version-first model, immutable snapshots, crate boundaries all coherent.
3. **Sync directly calls repo functions (pragmatic deviation from plan).** Plan says "calls API" but implementation calls repo directly. One-code-path preserved since sync uses same `versions::create`.
4. **Webhook runs in-process** (efficient, non-blocking via tokio::process::Command, but git pull blocks handler).
5. **Cross-board integrity not enforced at schema level** (application validation sufficient for v1).
6. **`scores_equal` function duplicated** in `versions.rs` and `execute.rs`.
7. **Docker image includes git** (intentional for webhook handler).
8. **Test coverage strong** -- 84 Rust integration tests + 14 Docker smoke tests.
9. **Cursor pagination correctly implemented** with `(created_at, id)` composite cursors.
10. **Graceful shutdown handles SIGINT + SIGTERM** (correct for Docker).
11. **Rate limiting layered before CORS** (OPTIONS preflight not rate-limited -- correct).
12. **Default config baked at /etc with env:DATABASE_URL** -- clean approach, secrets stay out of image.
