# Exhaustive Code Review — Round 3 (Convergence Verification)

**Reviewer:** Claude Opus 4.6
**Date:** 2026-02-23
**Scope:** Full codebase review of riley_leaderboards (3 crates, 6 migrations, integration tests, Docker/deploy config)
**Context:** R1 had 3 majors + 8 minors; R2 had 0 majors + 2 minors. This is R3, targeting convergence (second consecutive clean round).

---

## Review Methodology

Reviewed all source files across the three crates:
- `riley-leaderboards-core`: config, db, error, models, repo (boards, collections, entries, export, realtime, references, scores, versions), sync (parse, execute)
- `riley-leaderboards-api`: auth, error, lib (router, health, serve, shutdown), metrics, openapi, outbound_webhooks, routes (boards, collections, entries, references, scores, versions, webhooks), sse
- `riley-leaderboards-cli`: main.rs

Also reviewed: migrations (001-006), Dockerfile, docker-compose files, integration tests, example config.

Cross-referenced all findings against `planning/reviews/v2/review_notes_README.md` to avoid re-flagging documented decisions.

---

## R2 Fix Verification

All R2 fixes verified correct:

1. **Collections no-op guard (R2 Minor #1):** The no-op PATCH check is now in the route handler (`routes/collections.rs`) before calling the repo, matching the boards pattern. The repo `update()` function correctly uses a transaction with `FOR UPDATE`. No regression.

2. **Health response component status (R2 Minor #2):** The health handler now returns `{"status": "ok", "postgres": "ok"}` and conditionally adds `"redis": "ok"` when Redis is configured and reachable. The integration test's `jq -r '.redis // "absent"'` will now correctly find the field when Redis is running. No regression.

3. **ConnectionGuard underflow documented (R2 Note #3):** Documented in review_notes_README.md Phase 8 section. Accepted.

---

## Findings

### Major

None.

### Minor

None.

### Notes

#### 1. `scores_equal()` defined in `repo/mod.rs` — duplication resolved

In earlier review rounds, `scores_equal()` was noted as duplicated between `versions.rs` and `sync/execute.rs`. The current code has a single definition in `repo/mod.rs` (line ~17223) with `pub(crate)` visibility, and both `versions::diff` and `sync::execute::placements_changed` call it via `super::scores_equal()` and `crate::repo::scores_equal()` respectively. This duplication concern from the carried minors list is resolved in the current code.

#### 2. Webhook handler now requires `ref` field (pre-existing concern addressed)

The review_notes_README.md documents that the GitHub webhook handler "proceeds without branch check when `ref` is absent." Examining the current code (`routes/webhooks.rs`, lines 6760-6768), the handler now returns 400 with `"missing 'ref' field in push payload"` when `ref` is absent. This pre-existing concern documented in Phase 4 notes appears to have been addressed in a prior fix round, though the review_notes_README.md still carries the old note. This is a documentation staleness issue, not a code issue.

#### 3. `ServiceUnavailable` error handling is consistent

The `error.rs` API error handler now returns `"service temporarily unavailable"` for `ServiceUnavailable` errors (line 4761), which avoids leaking Redis connection details to clients. This addresses the Phase 5 review note about ServiceUnavailable passing error details — the current code sanitizes the message. The review_notes_README.md note about this is now slightly stale (it says the full message is passed, but the error handler now returns a generic string).

#### 4. All documented tradeoffs verified as still accurate

The following documented decisions in review_notes_README.md were verified against the current code:
- validate_aud = false: confirmed in auth.rs line 4684
- JWKS refresh task tracked via TaskTracker: confirmed in auth.rs lines 4471-4491
- Sync skips accumulative boards: confirmed in execute.rs line 18549
- CLI webhook delivery uses join_all: confirmed in main.rs line 14707
- No-op PATCH fires webhook for boards: confirmed, but now has no-op guard in route handler (lines 5689-5698) that short-circuits before the webhook fire
- Redis keyspace not namespaced: now uses configurable prefix (config.rs `key_prefix`, realtime.rs key functions) — the note is slightly outdated but the concern is addressed
- Broadcast channel buffer 256: confirmed in config.rs default
- RwLock poisoning unwrap(): confirmed in sse.rs, documented as accepted
- OpenAPI error annotations approximate: confirmed, accepted
- Board slug as metric label bounded: confirmed, accepted

---

## Summary

| Severity | Count | Details |
|----------|-------|---------|
| Major    | 0     | -- |
| Minor    | 0     | -- |
| Note     | 4     | Documentation staleness in review_notes_README.md (non-blocking), resolved prior duplication concern, consistent error handling |

---

## Convergence Assessment

R2 had 0 majors and 2 minors (both fixed). R3 has 0 majors and 0 minors. This constitutes **2 consecutive rounds with 0 majors**, meeting the convergence criteria.

The codebase is in good shape. The R1 fixes (metric label cardinality, JWKS task tracking, OpenAPI annotation corrections, FOR UPDATE on collections) are all correctly implemented and verified. No regressions were introduced. The remaining notes are about minor documentation staleness in review_notes_README.md where some entries describe concerns that have since been addressed — these are informational and do not require code changes.

**Verdict: CONVERGED.** The exhaustive review cycle is complete.
