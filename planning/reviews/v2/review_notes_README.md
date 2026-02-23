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

## Phase 3: Outbound Webhooks

### CLI webhook deliveries may be lost on process exit (accepted tradeoff)

The CLI commands (sync, delete-board, import) use `outbound_webhooks::fire()`
which internally calls `tokio::spawn()`. When `main()` returns, the tokio
runtime shuts down and cancels in-flight spawned tasks. For CLI commands this
means webhook deliveries to slow servers may be lost. This is accepted:
webhooks are best-effort notifications, and CLI usage is infrequent. If
reliability from CLI matters, collect JoinHandles and await them.

### Board update webhook fires on no-op PATCH (accepted)

A PATCH request with all null/absent fields produces a no-op UPDATE but still
fires a `board.updated` webhook. This is cosmetically noisy but consumers
should be idempotent. Filtering no-op updates would add complexity for minimal
benefit.

### Sync-created boards don't fire board.created webhooks (intentional)

When `sync_dir()` encounters a new board, it creates it in the DB but does not
fire a `board.created` outbound webhook. Sync primarily creates versions, and
the `version.created` webhook fires for those. Firing `board.created` from sync
would require refactoring the core sync module to return additional metadata
about what was created vs. updated. Accepted as a reasonable simplification.

### Webhook payload timestamp is wall-clock, not DB timestamp (accepted)

`WebhookPayload.timestamp` uses `chrono::Utc::now()` at fire time, not the
database `created_at` timestamp. The difference is milliseconds. Using DB
timestamps would require threading timestamps through the fire() API. Accepted.

### home_dir() only checks $HOME (accepted)

`config.rs home_dir()` only checks the `HOME` environment variable. This
doesn't work on Windows (which uses USERPROFILE), but the project targets
Linux containers (Dockerfile). Accepted.

## Phase 4: Board Collections

### No outbound webhook events for collection CRUD (intentional)

Board CRUD fires webhooks but collection CRUD does not. Collections are
organizational groupings, not data-bearing entities. Static site rebuilds
triggered by collection changes are unlikely. If needed, collection events
can be added in a future phase.

### CollectionBoardEntry omits entry_count (intentional)

The `GET /collections/:slug` response includes `latest_version` per board but
not `entry_count`. The v2 plan did not specify `entry_count` in the response
example. Consumers who need entry counts can fetch individual board details.
This avoids an extra subquery per board in the collection query.

### Correlated subquery for latest_version is fine at current scale

`get_with_boards()` uses `(SELECT MAX(v.version_number) ...)` per board. The
`versions` table has a unique index on `(board_id, version_number)` so this is
an index-only scan. For collections with hundreds of boards, a lateral join
would be more efficient, but current scale doesn't warrant the complexity.

### GitHub webhook handler proceeds without branch check when `ref` is absent (pre-existing)

The webhook `github` handler only filters by branch when `ref` is present in the
payload. If `ref` is absent, sync proceeds unconditionally. This is a pre-existing
behavior from Phase 1, not a Phase 4 issue. It could be tightened to return 400
for malformed payloads, but in practice GitHub always sends `ref` in push events.
