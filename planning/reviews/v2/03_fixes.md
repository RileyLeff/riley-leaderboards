# Fixes for Review Round 2 — 2026-02-23

**Commit**: 9abb32e

## Fixed

### Major
1. **Config fail-open** → changed `(None, None)` branch in `from_config` to `anyhow::bail!` instead of `tracing::warn!` + `NoAuth`. Now rejects configs with `read_tokens` or `require_read_auth` set without admin auth.

### Minor
2. **Example config** → updated `riley_leaderboards.example.toml` to show `admin_token`, `read_tokens`, `require_read_auth`, and legacy `api_token` alias (commented out).
3. **from_config tests** → added 5 tests:
   - `auth_from_config_admin_token_and_api_token_mutual_exclusion`
   - `auth_from_config_legacy_api_token_works`
   - `auth_from_config_read_tokens_without_admin_is_error`
   - `auth_from_config_require_read_auth_without_admin_is_error`
   - `auth_from_config_none_is_no_auth`
4. **Error conversion** → replaced `anyhow::anyhow!("{e}")` with `anyhow::Error::from(e)` in `from_config` to preserve error chain context.

## Deferred (not Phase 2 scope)

- `scores_equal` duplication (versions.rs + sync/execute.rs) — cosmetic, defer to cleanup phase
- Sync parse doesn't validate dir name as slug — confusing error but not incorrect, defer
- Export/import loses entry metadata — enhancement, not a bug, defer
