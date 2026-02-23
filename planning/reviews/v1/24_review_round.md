# Review Round 24 — Phase 6 Exhaustive R1

**Date:** 2026-02-22
**Models:** Gemini, Claude (Codex unavailable)
**Context:** ~102k tokens
**Scope:** Full codebase review, focus on Phase 6 (File Sync)

## Findings

### Major

1. **Scored board sync produces redundant versions** [gemini]
   - File: `core/src/sync/execute.rs` (`placements_changed`)
   - `placements_changed` compares `current_p.position (Some(N))` vs `p.position (None)` for scored boards — DB stores derived positions but TOML has None. Creates false change detection on every sync.
   - Fix: only compare position when `p.position.is_some()`.

2. **Webhook does not `git pull` before syncing** [consensus]
   - File: `api/src/routes/webhooks.rs` (`github`)
   - The webhook receives a push event but reads stale local files instead of pulling the updated repo first. First invocation on a fresh deployment would fail.
   - Fix: add `git -C $repo_path pull` before calling `sync_dir`.

3. **Webhook does not filter by branch** [claude]
   - File: `api/src/routes/webhooks.rs` (`github`)
   - Any push to any branch triggers sync, including feature branches. Experimental board files could be synced to production.
   - Fix: extract `ref` from payload, compare against configurable `sync_branch` (default "main").

### Minor

1. **Sync does not update existing entry name/metadata** [claude]
   - File: `core/src/sync/execute.rs` (`sync_board`, entry loop)
   - When entry exists, sync skips it entirely. Name/metadata changes in TOML are silently dropped.
   - Fix: compare name/metadata and call `entries::update` when they differ.

2. **`placements_changed` does not detect entry name changes** [claude]
   - Related to #1 above. Entry names are entry-level, not placement-level, so the diff function is correct as-is. The fix in #1 handles the actual update.
   - No code change needed in `placements_changed`.

3. **`constant_time_eq` leaks length via early return** [claude]
   - File: `api/src/routes/webhooks.rs`
   - Not exploitable (HMAC hex digests are always 64 chars) but can use hmac's built-in `verify_slice` instead.
   - Fix: replace manual hex comparison with `mac.verify_slice(&decoded_bytes)`.

4. **Tier position uses 0-based indexing** [claude]
   - File: `core/src/sync/execute.rs` (tier_config construction)
   - Plan shows 1-based tier positions. Sync uses `enumerate()` which is 0-based.
   - Fix: use `(i + 1) as i32`.

5. **Example config missing `[sync]` section** [consensus]
   - File: `riley_leaderboards.example.toml`
   - Operators won't know what config keys to add for webhook setup.
   - Fix: add commented-out `[sync]` section.

6. **Webhook returns error details to caller** [claude]
   - File: `api/src/routes/webhooks.rs` (error branch)
   - Internal error details (file paths, DB errors) leaked in response body.
   - Fix: return generic "sync failed" message, keep `tracing::error!` for server-side logging.

7. **No body size limit on webhook endpoint** [claude]
   - File: `api/src/routes/webhooks.rs`
   - GitHub push payloads can be up to 25MB. No limit on request body.
   - Fix: add `DefaultBodyLimit::max(5MB)` to the webhook route.

8. **`sync_dir` short-circuits on first error** [gemini]
   - File: `core/src/sync/execute.rs` (`sync_dir`)
   - A malformed board.toml in one directory aborts all remaining boards.
   - Fix: collect per-board errors as `SyncAction::Failed`, continue syncing remaining boards.

### Notes

1. **Sync bypasses API layer** — Both models raised this. The direct-to-repo approach is simpler for v1 (no HTTP client, no self-auth, no circular deps). API-level concerns (auth, rate limiting) will need to be addressed in Phase 7+. Intentional deviation from plan, documented.

2. **Non-atomic across boards** — If one board fails, others may have already committed. Reasonable for v1 simplicity.

3. **TOML parsing permissive about unknown fields** — No `#[serde(deny_unknown_fields)]`. Forward-compatible but typos are silent. Documented tradeoff.

4. **Webhook doesn't check X-GitHub-Event header** — `ping` events would trigger unnecessary sync. Harmless but wasteful.

5. **`f64` comparison may produce false negatives for NaN** — `validate_placements` rejects NaN scores, so this cannot happen through normal code paths.
