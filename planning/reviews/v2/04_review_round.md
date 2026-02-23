# Review Round 2 — 2026-02-23

**Models**: Codex, Claude (Gemini failed, exit 13)
**Context**: ~170k tokens
**Scope**: Verify Round 1 fixes, find new issues

## Verification

All 4 Round 1 fixes verified correct by both models:
1. Config fail-open → bails correctly
2. Example config → shows v2 fields
3. from_config tests → 5 tests added
4. Error conversion → preserves chain

## Findings

### Major

1. **[codex-only] `required_role` without any auth mode → NoAuth**
   - File: `auth.rs`, `from_config` `(None, None)` branch
   - Setting `required_role = "admin"` without `jwks_url`/`admin_token` falls to NoAuth
   - Same class of fail-open as R1 finding
   - Fix: extend bail condition to include `auth.required_role.is_some()`
   - Commit: 8d06fa7

### Minor

2. **[codex-only] JWT `nbf` not validated**
   - File: `auth.rs`, `validate_jwt`
   - `validate_nbf = false` (jsonwebtoken default) allows tokens before their not-before time
   - Fix: `validation.validate_nbf = true`
   - Commit: 8d06fa7

3. **[claude-only] Missing from_config test: jwks_url + admin_token exclusion**
   - Added test `auth_from_config_jwks_and_admin_token_mutual_exclusion`
   - Commit: 8d06fa7

4. **[claude-only] Missing test for empty [auth] section**
   - Added test `auth_from_config_empty_auth_section_is_no_auth`
   - Commit: 8d06fa7

### Notes

5. Health/webhook endpoints correctly outside auth (both models confirm)
6. scores_equal duplication (deferred, not Phase 2)
7. Integration test doesn't test auth (acceptable, smoke test only)
