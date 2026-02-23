# Review Round 9 — Phase 3 Exhaustive R2 (2026-02-23)

**Models**: Claude (Codex rate-limited)
**Context**: ~173k tokens
**Scope**: Full codebase — Phase 3 outbound webhooks post-R1 fixes

## R1 Fix Verification

All 7 Round 1 findings verified correctly fixed in commit 5363e15.

## Findings

### Major

None.

### Minor

1. **reqwest::Client created per delivery** — New client on every invocation discards connection pool and TLS cache. [claude-only]
2. **unwrap_or_default() drops timeout on build failure** — `Client::default()` has no timeout, silently losing the 10s protection. [claude-only]

### Notes

3. CLI webhook deliveries may be lost on process exit (tokio::spawn + main returns).
4. Inbound webhook handler uses same commit message note for all synced boards.
5. Board update webhook fires even on no-op PATCH.
6. No score.snapshot event type (snapshot fires version.created).
7. home_dir() only checks $HOME (fine for Linux/macOS).
8. Test TCP receiver uses single read() call (fine for small payloads).
