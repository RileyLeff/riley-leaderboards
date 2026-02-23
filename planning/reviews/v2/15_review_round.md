# Review Round 15 — Phase 5 Exhaustive R1

**Date:** 2026-02-23
**Models:** Codex, Claude (Gemini failed — exit 13 on large stdin)
**Context:** ~200k tokens

## Findings

### Major

1. **[consensus] Board deletion does not clean up Redis keys** — `realtime::clear()` exists but is never called from API `boards::delete` or CLI `delete-board`. Orphaned keys remain, and slug reuse inherits stale scores.

2. **[consensus] Snapshot TOCTOU race: Redis read before Postgres lock** — `snapshot()` reads Redis sorted set before `FOR UPDATE` lock. Concurrent score submissions between read and lock can be missed. With `clear_on_snapshot`, those scores are permanently lost.

3. **[codex-only] Redis keyspace not namespaced** — Keys are `board:{slug}:...` with no prefix. Shared Redis instances can collide across deployments. *Demoted to minor: single-tenant deployment is the target.*

### Minor

1. **[consensus] FLUSHDB in tests is race-prone** — Tests use `FLUSHDB` on shared Redis. Parallel test execution can wipe other tests' data.

2. **[consensus] Redis runtime errors return 500, not 503** — Redis errors map to `Error::Internal` → HTTP 500. Plan specifies 503 for Redis failures.

3. **[codex-only] PATCH cannot upgrade board to realtime** — `UpdateBoard` has no `realtime`/`clear_on_snapshot` fields. Plan says boards should be upgradable later.

4. **[codex-only] Submit response shape differs by mode** — Realtime returns `{"ok": true}`, non-realtime returns AccumulatedScore object. Surprising for clients.

5. **[claude-only] Snapshot entry_id lookups are O(N) queries** — Each entry does a separate SELECT. Should batch with `slug = ANY($1)`.

6. **[claude-only] Health endpoint doesn't check Redis** — Returns "ok" even when Redis is down, misleading for monitoring.

7. **[claude-only] Snapshot comment misleading** — "always desc for snapshot" is confusing vs latest() which respects sort_direction.

8. **[claude-only] `unwrap()` on serde_json::to_value in handlers** — Panic risk in request handlers.

9. **[codex-only] Docker integration tests don't exercise Redis** — Smoke tests only cover non-realtime paths.

### Notes

1. Migration 005 constraints are correct (consensus).
2. Redis atomic pipelines are correctly used (consensus).
3. Route dispatch logic is correctly wired (consensus).
4. No Redis key TTL — keys persist forever (accepted for active boards).
5. `accumulated_scores` bypass on realtime upgrade is lossy (accepted).
6. Test coverage gaps: concurrent submit/snapshot, asc snapshot, delete+Redis, export/import, Redis mid-request failure.
