# v2 Review Notes

Persistent notes on architectural tradeoffs, design decisions, and things
future sessions should know. Prevents re-litigating settled decisions.

Carries forward relevant v1 notes from `planning/reviews/v1/review_notes_README.md`.

## Phase 1: Version Metadata

### Sync does not detect version_metadata-only changes (intentional)

If a user updates only `[version_metadata]` in rankings.toml without changing
placements, no new version is created. The metadata update is silently
discarded. This is intentional:

- Versions are immutable snapshots of rankings. Creating a version with
  identical placements just to update metadata violates the soul document's
  principle that "every edit to rankings creates a new version."
- Metadata is context *about* a version, not the version itself.
- Users should update rankings and metadata in the same commit.
- Metadata can always be set via the API when creating versions directly.

### Migration numbering diverges from plan (intentional)

The v2 plan groups version metadata + collections into migration 003. The
implementation correctly splits them — each phase gets its own migration. When
Phase 4 (collections) lands, it will be migration 004+.
