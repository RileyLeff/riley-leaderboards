# Phase 3 Review Round 1 — Fixes

**Date:** 2026-02-22

## Fixed

1. **Diff `from`/`to` validation** — Added checks: both must be >= 1, `from` must be < `to`. Commit 075c312.

## Noted (no fix needed)

- `since` with negative version numbers: harmless, returns all versions
- `HashMap<String, i32>` deserialization: Axum framework behavior, low priority
- Placement metadata not in diff: by design
- Response shape divergence: implementation naming is better than plan examples
