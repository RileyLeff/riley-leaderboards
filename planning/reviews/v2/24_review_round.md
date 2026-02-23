# Exhaustive Review Round 1 — Phase 7 Completion

**Date:** 2026-02-23
**Models:** Claude Opus 4.6 only (Codex rate-limited, Gemini exit 13)
**Context:** ~216k tokens (full codebase)

## Major Findings

None after triage. Two findings initially flagged as major were downgraded:

1. **Webhook endpoint rate limiting** — Downgraded to note. HMAC verification fails fast (before git ops), and the global rate limiter + sync mutex provide adequate protection.
2. **SSE `std::sync::RwLock` in EventBus** — Downgraded to note. Tokio docs recommend `std::sync::RwLock` for short critical sections. Locks are held for microseconds (HashMap lookup + broadcast send).

## Minor Findings

1. **Webhook error messages leak config details** — Fixed in `ec9d57f`. Error responses now return generic "webhook processing failed"; specifics logged server-side.
2. **Non-functional wildcard CORS example** — Fixed in `ec9d57f`. Changed `https://*.rileyleff.com` to `https://app.rileyleff.com` in example config.

## Notes

- Token hashing uses fixed HMAC key (acceptable pattern for constant-time comparison)
- No pagination on `since` and `history` endpoints (unbounded result sets)
- `effective_limits()` clones on every request (cheap, just integers)
- Webhook handler holds sync_mutex across git operations (by design, mitigated by 60s timeout)
- Nested lock acquisition in EventBus prune (consistent lock ordering, safe)
- Docker runtime installs git (required for sync feature)
- Redis/Postgres dual-write non-atomicity (documented limitation)
- Version pagination sorts by created_at (correlates with version_number)
