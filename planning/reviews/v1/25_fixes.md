# Fixes for Review Round 24 (Phase 6 R1)

**Date:** 2026-02-22

## Major Fixes

### 1. Scored board redundant versions (Major #1)
- **File:** `core/src/sync/execute.rs` (`placements_changed`)
- **Fix:** Changed position comparison from `if current_p.position != p.position` to `if p.position.is_some() && current_p.position != p.position`. For scored boards, the TOML omits position (None) while the DB stores derived positions (Some(N)). Now we only compare when the proposed value explicitly sets a position.

### 2. Webhook git pull (Major #2)
- **File:** `api/src/routes/webhooks.rs` (`github`)
- **Fix:** Added `tokio::process::Command::new("git").args(["-C", &repo_path, "pull"])` before calling `sync_dir`. Returns 500 with generic error if pull fails.

### 3. Webhook branch filtering (Major #3)
- **Files:** `api/src/routes/webhooks.rs`, `core/src/config.rs`
- **Fix:** Added `sync_branch: Option<String>` to `SyncConfig` (defaults to "main"). Webhook extracts `ref` from payload and compares against `refs/heads/{sync_branch}`. Mismatched branches return 200 with `{"ignored": true}`.

## Minor Fixes

### 4. Entry name/metadata updates (Minor #1)
- **File:** `core/src/sync/execute.rs` (`sync_board`)
- **Fix:** When an existing entry is found, compare name and metadata against the TOML values. Call `entries::update` when they differ.

### 5. hmac verify_slice (Minor #3)
- **File:** `api/src/routes/webhooks.rs` (`verify_signature`)
- **Fix:** Replaced manual hex comparison + `constant_time_eq` with `hex::decode` + `mac.verify_slice()`. Removed `constant_time_eq` function entirely.

### 6. Tier position 1-based (Minor #4)
- **File:** `core/src/sync/execute.rs` (tier_config construction)
- **Fix:** Changed `serde_json::json!(i as i32)` to `serde_json::json!((i + 1) as i32)`.

### 7. Example config [sync] section (Minor #5)
- **File:** `riley_leaderboards.example.toml`
- **Fix:** Added commented-out `[sync]` section with `repo_path`, `webhook_secret`, and `sync_branch`.

### 8. Generic webhook error response (Minor #6)
- **File:** `api/src/routes/webhooks.rs` (error branch)
- **Fix:** Changed `format!("sync failed: {e}")` to `"sync failed"`. Internal details still logged via `tracing::error!`.

### 9. Webhook body size limit (Minor #7)
- **File:** `api/src/lib.rs` (`build_router`)
- **Fix:** Added `.layer(axum::extract::DefaultBodyLimit::max(5 * 1024 * 1024))` to the webhook route (5MB limit).

### 10. sync_dir continues on per-board errors (Minor #8)
- **File:** `core/src/sync/execute.rs` (`sync_dir`)
- **Fix:** Added `SyncAction::Failed { error: String }` variant. `sync_dir` now catches per-board errors, logs them with `warn!`, and includes them in results. Remaining boards continue to sync. CLI also handles the new variant.

### 11. Webhook event type check (Note #4)
- **File:** `api/src/routes/webhooks.rs` (`github`)
- **Fix:** Added `X-GitHub-Event` header check. Returns `{"pong": true}` for ping events. Ignores non-push events.

## Test Updates
- Webhook test (`webhook_valid_signature_triggers_sync`) updated to use a proper git bare+clone repo setup so `git pull` succeeds. Added `x-github-event: push` header.

## All 73 tests passing.
