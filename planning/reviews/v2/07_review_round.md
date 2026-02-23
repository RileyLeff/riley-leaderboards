# Review Round 7 — Phase 3 Exhaustive R1 (2026-02-23)

**Models**: Claude (Codex rate-limited, Gemini unavailable)
**Context**: ~171k tokens
**Scope**: Full codebase — Phase 3 outbound webhooks

## Findings

### Major

1. **Secret resolution failure silently skips HMAC signing** — `outbound_webhooks.rs:fire()` uses `.and_then(|cv| cv.resolve().ok())` which swallows env var resolution errors, delivering unsigned webhooks when a secret was configured.

### Minor

2. **CLI `sync` command does not fire outbound webhooks** — only the GitHub webhook handler path fires `VersionCreated` after sync, not the CLI `sync` command.
3. **CLI `DeleteBoard` command does not fire outbound webhooks** — API delete handler fires `BoardDeleted` but CLI does not.
4. **`import_board` does not fire outbound webhooks** — board creation via import has no event hooks.
5. **New `reqwest::Client` allocated per delivery** — connection pool and TLS cache discarded each time.
6. **Glob patterns with `*` in non-trailing positions fail silently** — `*-rankings` treated as literal match, no validation at config parse time.
7. **Sync webhook handler uses raw input `note`** — should reflect stored note; minor inconsistency in edge case where fallback is used.

### Notes

8. Retry backoff [1,5,25] is 5^n not 2^n; unnecessary 25s sleep after final attempt.
9. `tokio::spawn` means in-flight deliveries lost on shutdown; no backpressure.
10. 4xx errors retried unnecessarily (will never succeed).
11. No negation patterns in board filter (reasonable simplification).
12. Test coverage gaps: `board.updated` integration, snapshot path, sync+outbound, retry behavior.
13. Sync-created boards don't fire `board.created` (inconsistent with API).
14. Payload timestamp is wall-clock (`Utc::now()`), not DB timestamp.
