# Review Round 30 — Phase 7 Exhaustive Review R1 — 2026-02-22

**Models**: Claude only (Codex produced no output file; Gemini command not found)
**Context**: ~120k tokens

## Findings

### Major

1. **[claude-only] Webhook `git pull` has no timeout — potential DoS.** The `git pull` command in the webhook handler (webhooks.rs) runs with no timeout, meaning a slow upstream repo could block the handler indefinitely.

2. **[claude-only] JWKS refresh failure silently serves stale keys with no expiration.** When `JwksCache::refresh()` fails in the background task (auth.rs), old keys remain cached indefinitely. A compromised key rotated out at the identity provider will continue to be accepted. No TTL or max-age on cached keys.

3. **[claude-only] `f64` score comparison uses direct equality (`!=`).** In `placements_changed()` (execute.rs) and `version_diff()` (versions.rs), scores are compared using `!=` on `Option<f64>`. While NaN/Infinity are rejected at input, this is fragile if the codebase evolves.

### Minor

1. **[claude-only] `extract_bearer_token` is case-sensitive for "Bearer" scheme.** Per RFC 7235, the scheme is case-insensitive. `strip_prefix("Bearer ")` won't match `bearer` or `BEARER`.

2. **[claude-only] Tiered boards can be created without `tier_config`.** `validate_tier_config` returns `Ok(())` when tier_config is `None`, meaning placements can use arbitrary tier strings with no validation.

3. **[claude-only] No pagination on list endpoints.** All list endpoints return all records — could return very large responses.

4. **[claude-only] `diff` endpoint loads both full versions into memory.** For boards with thousands of entries, could be memory-intensive.

5. **[claude-only] `since` endpoint returns versions without placements.** Inconsistent with `get_by_number` and `get_latest` which return `VersionWithPlacements`.

6. **[claude-only] `cors_origins` config field parsed but never used.** No CORS middleware is configured despite config field existing.

7. **[claude-only] Webhook event type check happens after HMAC verification.** Minor performance concern — HMAC computed before determining event should be ignored.

8. **[claude-only] Webhook endpoint can leak config state to unauthenticated callers.** Different 500-level errors depending on config state.

9. **[claude-only] Test cleanup uses unparameterized `format!()` for DROP SCHEMA SQL.** Schema names are hardcoded constants but pattern could be problematic if copied.

10. **[claude-only] `behind_proxy` config field parsed but never used.**

11. **[claude-only] `board_type` and `accumulative` immutability enforced by omission.** Client sending these in PATCH gets no error — fields silently ignored.

### Notes

- Auth token HMAC approach is sound for constant-time API token comparison
- JWKS supports only RSA keys (no EC/EdDSA) — reasonable v1 scope limitation
- `validate_aud = false` is an explicit tradeoff for single-purpose deployments
- Version number serialization via `FOR UPDATE` lock is well-designed
- Schema isolation implementation is robust
- Sync module is intentionally non-transactional at directory level
- Test coverage is thorough; main gap is concurrent version creation tests
