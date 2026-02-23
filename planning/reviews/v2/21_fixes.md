# Phase 6 R1 Fixes

**Commit:** 2ec627c

## Major Fixes

### M1: Atomic connection limit — FIXED
Replaced check-then-increment with `fetch_add` first, check if over limit, `fetch_sub` to undo. Also upgraded all atomic operations from `Ordering::Relaxed` to `Ordering::AcqRel` (modifications) and `Ordering::Acquire` (reads). This addresses both M1 and m4.

### M2: SSE events from webhook sync — FIXED
Added `event_bus.publish_version()` call in the webhook handler's sync results loop, right after the outbound webhook fire. SSE subscribers now receive version.created events for git-synced boards.

### M3: CLI sync SSE events — DOCUMENTED
CLI sync runs without a server, so no SSE subscribers exist. Documented in review_notes_README.md. No code change needed.

## Minor Fixes

### m6: Example config SSE fields — FIXED
Added commented SSE config fields to `riley_leaderboards.example.toml`.

### m8: Debounce inside channel check — FIXED
Moved debounce check inside the channel existence guard (`let Some(tx) = ...`). Debounce timestamps are no longer written when no subscribers exist.

### m4: Ordering::Relaxed → AcqRel — FIXED (with M1)

## Accepted / Documented (review_notes_README.md)

- m2: Redundant "type" in JSON — accepted (convenience for JSON-only clients)
- m3: score.updated missing "position" — intentional (expensive to compute)
- m5: No timeout — deferred (reverse proxy typically handles this)
- m7: RwLock unwrap() — accepted (poisoning is extremely unlikely)
- m1: Unbounded HashMap growth — noted, not critical at expected scale
