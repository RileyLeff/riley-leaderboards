# Phase 9 R1 Fixes

**Date**: 2026-02-23
**Commit**: 392346c

## Major Fixes

1. **Caddy path prefix stripping** (consensus) -- Changed `handle` to `handle_path` in `Caddyfile.snippet` so the `/api/leaderboards` prefix is stripped before proxying to the service. Without this, every proxied request would 404 in production.

2. **Webhook concurrency protection** (claude-only) -- Added `sync_mutex: tokio::sync::Mutex<()>` to `AppState`. The webhook handler acquires this mutex before starting `git pull` + `sync_dir`, preventing concurrent git operations from corrupting the worktree.

## Minor Items (deferred to post-R2)

Items 1-12 from the R1 review are minor-severity and will be addressed after verifying the major fixes in R2. Several are carried from Phase 8 (import validation, behind_proxy, etc.).
