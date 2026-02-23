# Review Round 2 — 2026-02-23

**Models**: Codex, Claude (Gemini failed, exit 13)
**Context**: ~159k tokens
**Scope**: Exhaustive review of full codebase after Phase 2 (Read-Only API Keys)

## Findings

### Major

1. **[consensus] Config fail-open: read_tokens/require_read_auth without admin auth degrades to NoAuth**
   - File: `crates/riley-leaderboards-api/src/auth.rs`, `from_config` `(None, None)` branch
   - When `[auth]` is present with `read_tokens` or `require_read_auth` but no `admin_token`/`jwks_url`, the code warns and falls to `NoAuth`, silently opening all endpoints including writes
   - Should be a startup error — this config is always a mistake
   - Found by: Claude (as minor) + Codex (as major) — using highest severity

### Minor

2. **[consensus] Example config doesn't show v2 auth fields**
   - File: `riley_leaderboards.example.toml`
   - Still only shows legacy `api_token`, missing `admin_token`, `read_tokens`, `require_read_auth`
   - Found by: Claude + Codex

3. **[consensus] No tests for `from_config` behavior**
   - Tests construct `AuthMode` variants directly — no tests go through `from_config`
   - Missing: mutual exclusion error, legacy `api_token` alias, misconfig bail
   - Found by: Claude + Codex

4. **[codex-only] Error conversion in `from_config` stringifies, drops chain context**
   - File: `crates/riley-leaderboards-api/src/auth.rs`, line ~61
   - `anyhow::anyhow!("{e}")` loses the error chain; should use `anyhow::Error::from(e)` or `.context()`

### Notes

5. **[claude-only] Read token on write endpoint in JWT mode gives confusing "invalid JWT" error**
   - A read-only API token sent on a POST in JWT mode falls through to JWT validation and gets "invalid JWT" instead of "read tokens cannot write"
   - Not a security issue, just a UX concern — documenting as intentional for now

6. **[claude-only] scores_equal duplicated in versions.rs and sync/execute.rs**
   - DRY violation, not Phase 2 scope — defer to later cleanup

7. **[claude-only] Sync parse doesn't validate dir name as slug early**
   - Error shows up later during create, not during parse — confusing but not incorrect
   - Not Phase 2 scope

8. **[claude-only] Export/import loses entry-level metadata**
   - Not Phase 2 scope — track for future enhancement

9. **Webhook ref field missing skips branch check** (previously flagged in v1)
   - GitHub always includes `ref` in push events; HMAC verification ensures payload authenticity
   - Low risk, documenting as acceptable

## False Positives

- Claude FINDING 5.1 (MAJOR: "no tests for require_read_auth"): Tests `auth_require_read_auth_blocks_unauthenticated_reads` and `auth_jwt_require_read_auth` already exist
- Claude FINDING 5.4 (MINOR: "no tests for read tokens in JWT mode"): Test `auth_jwt_read_token_can_read_but_not_write` exists
- Claude FINDING 2.3 (JWT audience disabled): Already documented as intentional in v1 Phase 7 review notes
- Claude FINDING 2.4 (HMAC fixed key): Already documented as intentional in v1 Phase 7 review notes
