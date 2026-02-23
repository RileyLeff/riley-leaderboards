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

## Phase 2: Read-Only API Keys

### validate_aud = false is intentional

JWT validation does not enforce audience (`validate_aud = false`). This is
intentional for a single-tenant deployment where the leaderboards service is
the only audience. If multi-tenant support is added in the future, audience
validation should be reconsidered.

### required_role omission allows any valid JWT to write (intentional)

When JWT mode is configured without `required_role`, any valid JWT can perform
write operations. This is the intended behavior — `required_role` is an optional
additional constraint, not a requirement. Deployments that want to restrict
writes to specific roles should set `required_role`.

### CORS wildcard origins are operational, not a code concern

CORS origin values come from config, not code. The code correctly applies
whatever origins are configured. Restrictive origins are an operational best
practice documented in the example config.

### Carried minors (deferred, not Phase 2 scope)

These minors have been flagged across multiple review rounds and are accepted
as deferred items:

- `scores_equal()` duplication (versions.rs + sync/execute.rs) — cosmetic
- Tier config duplicate key validation — edge case, defer to cleanup
- Plan safety limits (max_entries, max_versions, etc.) — operational hardening
- CASCADE FK on placements.entry_id — defense-in-depth tradeoff, accepted
- Integration tests don't exercise Caddy deployment path — accepted gap
