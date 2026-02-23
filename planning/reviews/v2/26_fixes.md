# Fixes — Exhaustive Review Rounds 1-2

## Round 1 Fixes (commit `ec9d57f`)

1. **Webhook error message leaks** — Changed "webhook secret not configured" and "sync repo_path not configured" to generic "webhook processing failed". Added `tracing::error!` for server-side logging.
2. **Wildcard CORS example** — Changed `https://*.rileyleff.com` to `https://app.rileyleff.com` in example config since CORS layer does literal matching.

## Round 2 Fixes (commit `22e1959`)

1. **Safety limit coverage** — `max_versions_per_board` now checked inside transactions (after `FOR UPDATE`) in `versions::create`, `scores::snapshot`, and `realtime::snapshot`. Eliminates TOCTOU race and ensures limits apply to snapshot paths.
2. **Entry count limits on snapshot** — Both snapshot functions now check `max_entries_per_version`.
3. **Metadata size validation** — Added to collection create/update, entry create/update, and snapshot handler. Shared `check_metadata_size` helper extracted to `routes/mod.rs`.
4. **Sync bypass** — Sync paths pass `None` for limits (intentional — operator-initiated operations are unbounded).
