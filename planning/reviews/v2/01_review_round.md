# Review Round 1 — Phase 1 (Version Metadata)

**Date:** 2026-02-23
**Models:** Claude Opus 4.6 only (Codex unavailable, Gemini unavailable)
**Context:** ~153k tokens

## Findings

### Major
None.

### Minor

1. **Sync does not detect version_metadata changes** [claude-only]
   - `crates/riley-leaderboards-core/src/sync/execute.rs`, `sync_board`
   - If user updates only `[version_metadata]` in rankings.toml without changing placements, no new version is created, metadata update is silently discarded.
   - **Resolution:** Documented as expected behavior. Creating versions with identical placements just to update metadata violates the immutability principle. Users should update rankings and metadata in the same commit.

### Notes

2. Migration 003 correctly splits from plan's suggested bundling with collections [claude-only]
3. No size/depth validation on version metadata JSONB — pre-existing pattern from v1 [claude-only]
4. Test coverage is thorough — 5 tests covering all paths [claude-only]
5. SnapshotInput metadata is backward-compatible addition [claude-only]
6. SELECT * usage works but has schema-drift fragility — existing pattern [claude-only]
7. Export/import handles metadata correctly with skip_serializing_if [claude-only]
8. TOML-to-JSON conversion correctly reuses existing toml_to_json [claude-only]
