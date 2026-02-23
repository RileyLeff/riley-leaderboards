# Review Round 17 — Phase 5 Exhaustive R2 (Convergence Check)

**Date:** 2026-02-23
**Models:** Claude (Codex hit rate limits, Gemini failed exit 13)
**Context:** ~197k tokens
**Commit:** efba405

## Findings

### Major

None.

### Minor

1. **[claude-only] Snapshot `clear_on_snapshot` failure returns error after Postgres commit** — `realtime.rs:246-257`: When `clear_on_snapshot` is true and Redis DEL fails after `tx.commit()` succeeds, the function returns `Err(ServiceUnavailable)`. The client sees 503 (may retry creating duplicate), webhook doesn't fire, and Redis state isn't cleared. Should use `let _ =` / log pattern like board deletion.

### Notes

1. **[claude-only] N1: No integration test for health endpoint with Redis** — health_test.rs only covers `redis: None`. Adding a test with Redis configured would improve confidence.

2. **[claude-only] N2: ServiceUnavailable leaks Redis error details** — Unlike Database errors which return generic "internal server error", ServiceUnavailable passes full error string to clients. Redis connection errors may contain connection details.

3. **[claude-only] N3: Explicit rollback on empty snapshot is unnecessary** — `realtime.rs:170-175`: sqlx auto-rolls-back on drop. The explicit `tx.rollback()` is not harmful, just unnecessary. Style observation only.

4. **[claude-only] N4: Redis key computed before FOR UPDATE re-fetch** — `realtime.rs:150-161`: Redis keys built from caller's board slug before the locked re-fetch. Safe because slugs are immutable (no rename endpoint exists).
