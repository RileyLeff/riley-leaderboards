# Phase 4 Review R1 — Fixes

**Date:** 2026-02-22
**Commit:** 47f5ba5

## Fixes Applied

### 1. URI validation (Minor #1)
Added `validate_uri` function in `repo/references.rs`:
- Rejects empty strings
- Enforces max 2048 character limit
- Called in `references::create` before database interaction
- New test: `reference_empty_uri_returns_400`

### 2. Version number in response (Minor #2)
Updated `BoardReference` model and queries:
- Added `pinned_version_number: Option<i32>` field to `BoardReference` struct
- `create` query uses CTE + LEFT JOIN to return version number immediately
- `list` query uses LEFT JOIN to include version number
- Updated test assertions to verify `pinned_version_number` presence

### 3. Label length validation (Minor #3)
Added `validate_label` function in `repo/references.rs`:
- Validates label length <= 256 chars (only when label is provided)
- Called in `references::create`
- New test: `reference_label_too_long_returns_400`

## Test Results
All 53 tests passing after fixes.
