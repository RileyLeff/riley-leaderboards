# Phase 8 Exhaustive Review — Round 1

**Date**: 2026-02-23
**Models**: Claude Opus 4.6 (Gemini failed exit 13, Codex not attempted)
**Context**: ~133k tokens

## Findings

### Major

1. **Import not transactional** — `import_board()` has no transaction; partial imports leave broken state. [claude-only]
2. **Import bypasses validation** — Direct SQL inserts skip `validate_placements()`. [claude-only]
3. **Import doesn't derive scored positions** — Scored board imports don't call `derive_scored_positions()`. [claude-only]
4. **Webhook route always registered** — No check for sync config presence; returns 500 when config missing. [claude-only]

### Minor

5. `scores_equal()` duplicated in versions.rs and execute.rs [claude-only]
6. Import uses first-seen entry name, ignoring later updates [claude-only]
7. Pagination cursor format has no direction encoding [claude-only]
8. Plan-specified safety limits not implemented [claude-only]
9. `behind_proxy` config parsed but unused — rate limiting broken behind proxies [claude-only]
10. Rate limiting applies to /health endpoint [claude-only]
11. CORS layer ordering may rate-limit preflight requests [claude-only]
12. Export N+1 query pattern [claude-only]
13. `diff` endpoint from==to handled correctly (not a bug) [claude-only]
14. Webhook `git pull` doesn't specify branch explicitly [claude-only]
15. No pagination tests for versions or references [claude-only]
16. Test cleanup SQL doesn't use `quote_identifier()` [claude-only]
17. Plan says sync calls API; implementation calls repo directly [claude-only]

### Notes

18-28. PUT in CORS unused, wildcard CORS implications, verbose cursor format, JWKS cache replaces keys on empty fetch, export excludes references/accumulated scores, SnapshotInput allows empty body, board_type immutable by omission, entry metadata lost on import, `[boards]` config not implemented, sync_branch default undocumented, positive observations about code quality.
