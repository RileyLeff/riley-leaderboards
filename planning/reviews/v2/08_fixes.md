# Fixes for Review Round 7 — Phase 3 R1 (2026-02-23)

**Commit**: 5363e15

## Major Fixes

1. **Secret resolution failure now skips delivery** — Changed `fire()` to match on `cv.resolve()` explicitly: `Err` logs an error and `continue`s instead of silently delivering unsigned. (outbound_webhooks.rs)

## Minor Fixes

2. **CLI `sync` fires outbound webhooks** — Added `VersionCreated` webhook firing for each Created/Updated sync result in the CLI Sync command. (main.rs)

3. **CLI `DeleteBoard` fires outbound webhooks** — Fetches board info before deletion, fires `BoardDeleted` event. (main.rs)

4. **`import` fires outbound webhooks** — Fires `BoardCreated` event after successful import. (main.rs)

5. **reqwest::Client timeout at builder level** — Client is now built with `.timeout(10s)` and reused across retries within a delivery.

6. **Glob pattern validation at config parse time** — `validate_webhook_board_patterns()` rejects patterns with `*` in non-trailing positions (e.g., `*-rankings`, `dc-*-rankings`). 3 new unit tests.

7. **4xx errors no longer retried** — `deliver()` now returns immediately on client errors (4xx) since retrying won't help.

8. **Unnecessary final sleep removed** — Delays changed to `[1, 5, 0]`; sleep is skipped when delay is 0.

## Notes (no action)

- #7 sync note: The GitHub webhook handler uses the git commit message as note, which matches what sync stores. The edge case where `note` is `None` and the DB stores a fallback is CLI-only; CLI now fires webhooks too but the note is the CLI-provided one, which is correct.
- #8-14: Acknowledged as reasonable tradeoffs for the current scale.
