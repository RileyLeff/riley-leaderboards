# Fixes for Review Round 2 (R2) — 2026-02-23

**Commit**: 8d06fa7

## Fixed

### Major
1. **`required_role` without any auth mode → NoAuth** — Extended bail condition in `from_config` `(None, None)` branch to include `auth.required_role.is_some()`. Setting `required_role = "admin"` without `jwks_url` or `admin_token` now correctly rejects the config at startup instead of falling to NoAuth.

### Minor
2. **JWT `nbf` not validated** — Set `validation.validate_nbf = true` in `validate_jwt`. Tokens are now rejected if presented before their not-before time.
3. **Missing from_config test: jwks_url + admin_token exclusion** — Added test `auth_from_config_jwks_and_admin_token_mutual_exclusion`.
4. **Missing test for empty [auth] section** — Added test `auth_from_config_empty_auth_section_is_no_auth`.
5. **Missing test for required_role without auth mode** — Added test `auth_from_config_required_role_without_auth_mode_is_error`.
