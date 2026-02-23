# Fixes for Review Round 17 (Phase 5 R2)

**Commit:** 758d7a6

## Minor Fixes

1. **clear_on_snapshot Redis failure now non-fatal** — `realtime.rs:248-257`: After `tx.commit()` succeeds, Redis DEL is now best-effort. Errors are logged via `tracing::error!` instead of propagating as `ServiceUnavailable`. This prevents: (a) client seeing 503 for a successfully committed snapshot, (b) duplicate snapshots from retries, (c) suppressed outbound webhooks.

## Notes (acknowledged, no action)

1. **No health test with Redis** — Deferred. The health endpoint Redis PING code is simple and manually verified. Adding a test requires Redis in the health_test.rs setup.

2. **ServiceUnavailable leaks Redis error details** — Acknowledged. The messages are operator-facing ("Redis is required for realtime boards but not configured"). Redis crate errors could contain connection details, but this is acceptable for a self-hosted service. Can be tightened later if needed.

3. **Explicit rollback on empty snapshot** — Style preference, not a bug. Kept for clarity of intent.

4. **Redis key computed before FOR UPDATE re-fetch** — Safe because slugs are immutable (no rename endpoint). Documented for awareness if a rename feature is ever added.
