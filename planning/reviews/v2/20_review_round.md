# Phase 6 Exhaustive Review — Round 1

**Date:** 2026-02-23
**Models:** Claude only (Codex rate-limited, Gemini exit 13)
**Context:** ~206k tokens
**Scope:** Full codebase, focused on Phase 6 (SSE live updates)

## Major

### M1 [claude-only] Race condition in connection limit check-then-increment
- **File:** `sse.rs:75-93`
- **Issue:** `subscribe()` loads count, checks limit, then increments — not atomic. Under concurrency, two threads can both pass and exceed `max_connections`.
- **Fix:** Use `fetch_add` first, check result, `fetch_sub` to undo if over limit.

### M2 [claude-only] SSE events not published for webhook-triggered sync
- **File:** `routes/webhooks.rs`
- **Issue:** GitHub push → sync_dir → version created, but `publish_version()` never called on EventBus. SSE subscribers miss version.created from git sync.
- **Fix:** Add EventBus publish after sync results loop.

### M3 [claude-only] SSE events not published for CLI sync command
- **File:** `main.rs` sync command
- **Issue:** CLI sync doesn't create EventBus or publish events. Less impactful (no server running = no subscribers), but should be documented.
- **Fix:** Document behavior. CLI sync runs without server, so no SSE subscribers exist.

## Minor

### m1 Unbounded HashMap growth in EventBus
- **File:** `sse.rs:51-56`
- **Issue:** `channels` and `last_score_event` HashMaps never prune entries.
- **Fix:** Periodic cleanup of channels with 0 receivers and stale debounce entries.

### m2 Redundant "type" field in SSE JSON
- **File:** `sse.rs:24-46`
- **Issue:** `#[serde(tag = "type")]` adds "type" to JSON data, duplicating the SSE `event:` field.
- **Fix:** Document or remove.

### m3 score.updated missing "position" field from plan
- **File:** `sse.rs:33-37`
- **Issue:** Plan specifies `position` in event; implementation has `entry_name` instead.
- **Fix:** Update plan to reflect implementation (position is expensive to compute).

### m4 Ordering::Relaxed may be insufficient
- **File:** `sse.rs:75,93,146,156`
- **Fix:** Use AcqRel for modifications, Acquire for reads (fix alongside M1).

### m5 No SSE timeout / max connection duration
- **File:** `sse.rs`
- **Issue:** Plan says 30min inactivity timeout; implementation has none.
- **Fix:** Add timeout wrapper or document as deferred.

### m6 Example config missing SSE fields
- **File:** `riley_leaderboards.example.toml`
- **Fix:** Add commented SSE fields.

### m7 RwLock unwrap() on poisoned lock
- **File:** `sse.rs`
- **Fix:** Use `unwrap_or_else(|e| e.into_inner())` or document.

### m8 Debounce timestamp updated when no channel exists
- **File:** `sse.rs:122-131`
- **Fix:** Move debounce check inside channel existence check.

## Notes

- n1: Auth on SSE correctly follows read endpoint rules ✓
- n2: Broadcast buffer 256 is reasonable with default debounce
- n3: GuardedStream pattern is clean and idiomatic
- n4: Non-realtime scores correctly don't emit SSE events
- n5: Events fire after DB commit — correct ordering
- n6: score.updated uses submitted data not aggregated standings — defensible
- n7: Test cleanup pattern is standard
- n8: Integration tests use direct EventBus subscription, not HTTP stream reading

## Test Coverage Gaps

- T1: No end-to-end SSE stream content test (highest priority)
- T2: No SSE test with `require_read_auth = true`
- T3: No concurrent subscribe stress test
- T6: No SSE events during webhook-triggered sync test
