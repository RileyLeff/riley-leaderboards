# Phase 4 Exhaustive Review — Round 1

**Date:** 2026-02-22
**Models:** Gemini + Claude (Codex rate-limited)
**Context:** ~85k tokens
**Scope:** Phase 4 (References) implementation review

## Findings

### Major
None.

### Minor

#### 1. [consensus] URI not validated — empty strings accepted
**Files:** `crates/riley-leaderboards-core/src/repo/references.rs`
**Models:** Gemini, Claude
While `board_type` and `ref_type` are validated, the `uri` field was accepted without any checks. An empty string or an excessively long string could be persisted.
- **Fix:** Added `validate_uri` check (non-empty, max 2048 chars). Commit 47f5ba5.

#### 2. [consensus] Response contains UUID but not version number
**Files:** `crates/riley-leaderboards-core/src/models.rs`, `crates/riley-leaderboards-core/src/repo/references.rs`
**Models:** Gemini, Claude
The `BoardReference` struct returned `pinned_version_id` (UUID) but lacked the `pinned_version_number`. Clients would need a separate lookup to know which version a reference was pinned to.
- **Fix:** Added `pinned_version_number: Option<i32>` to `BoardReference`, updated `create` (CTE + LEFT JOIN) and `list` (LEFT JOIN) queries. Commit 47f5ba5.

#### 3. [claude-only] No label length validation
**Files:** `crates/riley-leaderboards-core/src/repo/references.rs`
**Models:** Claude
The `label` field was accepted without length checks. An excessively long label could be persisted.
- **Fix:** Added `validate_label` check (max 256 chars). Commit 47f5ba5.

### Notes

- **[SQL] Parameterized queries used throughout** — `repo/references.rs` correctly uses `sqlx` parameter binding for all inputs, mitigating SQL injection risks.
- **[API] Proper Error Mapping** — Route handlers correctly map `Error::NotFound` and `Error::Validation` to appropriate HTTP status codes.
- **[Data Integrity] Cross-board pinning prevented** — `references::create` resolves the version number within the context of the provided `board_id`.
- **[Tests] High Coverage** — Integration tests cover success paths, invalid types, nonexistent versions, and cascading deletions.
- **[Security] Board-scoped delete** — The `delete` operation requires both `reference_id` and `board_id`, preventing cross-board deletion.

## Convergence

| Round | Major | Minor | Models |
|-------|-------|-------|--------|
| R1    | 0     | 3     | Gemini + Claude |

Not yet converged. Need 2 consecutive rounds with 0 major bugs.
