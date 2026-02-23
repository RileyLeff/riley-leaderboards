# Exhaustive Review Round 2 — Phase 7 Completion

**Date:** 2026-02-23
**Models:** Claude Opus 4.6 only (Codex rate-limited, Gemini exit 127)
**Context:** ~216k tokens (full codebase)

## Major Findings

None.

## Minor Findings

All 4 fixed in `22e1959`:

1. **M1: Snapshot and sync bypass max_versions_per_board** — Added `max_versions: Option<i64>` parameter to `versions::create`, `scores::snapshot`, and `realtime::snapshot`. Checked inside transaction after `FOR UPDATE` lock. Sync paths pass `None` (operator-initiated).
2. **M2: TOCTOU race on version count check** — Moved count check from route handler (outside tx) into `versions::create` (inside tx, after `FOR UPDATE`). Now serialized with concurrent version creation.
3. **M3: Metadata size not enforced on collections/entries/snapshot** — Added `check_metadata_size` calls to collection create/update, entry create/update, and snapshot handler. Extracted shared helper to `routes/mod.rs`.
4. **M4: max_entries_per_version not enforced on snapshot** — Added `max_entries: Option<usize>` parameter to both snapshot functions. Checked after reading scores/Redis entries.

## Notes

- `VersionWithPlacements` flattening causes nil UUIDs at top level for realtime latest (documented convention)
- `FOR UPDATE` comment in `entry::delete` is misleading about what it serializes (FK constraint provides actual safety)
- `entry_id` nil in realtime latest (clients should use `entry_slug` for cross-referencing)
- Health endpoint reveals infrastructure topology (acceptable for monitoring endpoint)
