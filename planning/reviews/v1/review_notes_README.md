# Review Notes

Persistent notes on architectural tradeoffs, design decisions, and things
future sessions should know. Prevents re-litigating settled decisions.

## Phase 1

### Auto-migrate on serve is intentional
`serve` auto-runs migrations for development convenience. A `--skip-migrate`
flag may be added in Phase 9 (deployment) if needed. For now, `migrate` exists
as a standalone command for production use.

### Integer migration prefix is fine
Using `001_` prefix instead of timestamps. Solo project, no collision risk.

### `home_dir()` uses `$HOME` only
Correct for Linux/macOS target. No Windows support planned.

### `double precision` for scores
Acceptable tradeoff for game scores. If exact-precision use cases arise,
`numeric` can be considered for specific board types.

### No `updated_at` trigger yet
Columns exist with `DEFAULT now()` but no auto-update trigger. Application
code in Phase 2 will handle this — either explicit SET or a DB trigger.

### `board_references.uri` has no uniqueness constraint
Deliberate: the same URI can reference a board in different contexts (e.g.,
embedded vs cited). If accidental duplicates become an issue, add a UNIQUE
on `(board_id, uri, ref_type)` later.

### No test for public schema migration path
`migrate_default_schema` uses a custom schema to avoid polluting the shared
`public` schema. Acceptable tradeoff for test isolation.

### ref_type taxonomy: embed/citation/context (not blog_post/game/page)
The plan originally used entity-type values (blog_post, game, page) but the
implementation uses relationship-type values (embed, citation, context). The
implementation's taxonomy is stronger — it's more compositional (a blog post
can embed OR cite a board). Plan updated to match in round 2.

### Unix-specific signal handling is not cfg-gated
`shutdown_signal()` uses `tokio::signal::unix::signal` without `#[cfg(unix)]`.
The project targets Linux/macOS. If cross-platform support becomes a goal,
wrap in cfg guard.

### ConfigValue "env:" prefix is implicit
A value starting with "env:" is treated as an environment variable reference.
This should be documented for end users to avoid confusion.

### Integration tests may leak schemas on panic
If an assertion panics before cleanup runs, test schemas remain in the
database. Harmless in dev, but if it causes flaky tests, consider Drop guards.

### validate checks connectivity only, not schema existence
`connect_readonly` sets `search_path` to a potentially non-existent schema,
which PostgreSQL accepts silently. `validate` confirms the database is
reachable but does not verify the configured schema exists.

### Cross-board integrity enforced at application level (resolved in Phase 2)
Codex flagged that the schema allows cross-board data. Phase 2 resolves this
with application-level enforcement: version creation resolves entry slugs via
`WHERE board_id = $1 AND slug = $2`, ensuring placements always reference
entries belonging to the same board. Schema-level loophole exists (raw SQL
could bypass) but acceptable since the service is the only writer.

## Phase 2

### board_type and accumulative are immutable via PATCH (intentional)
`UpdateBoard` excludes `board_type` and `accumulative`. These are set at
creation time and cannot be changed. This aligns with the plan and soul doc.

### Nonexistent entry in version creation returns 400, not 404
When a placement references an entry slug that doesn't exist on the board,
this returns `Error::Validation` (400) not `Error::NotFound` (404). The entry
is not a standalone resource being looked up — it's a validation failure in
the context of version creation.

### Mixed explicit/implicit positions in ordered boards are rejected
When ordered board placements mix explicit and implicit positions, the resolved
positions (explicit or derived from array index) must be unique. Collisions
between explicit and implicit positions return 400.

### ON DELETE CASCADE on placements.entry_id is intentional layering
Application code rejects entry deletion when placements exist (409). The
schema-level CASCADE remains for board-level deletion (board → entries →
placements cascade correctly). Two-layer approach: app protects entry-level,
schema handles board-level.

### Safety limits (max_entries_per_version, etc.) are Phase 8
Plan specifies limits but these are operational concerns, not core CRUD.
Will be implemented in Phase 8 (polish/deployment).

### N+1 queries in version creation are acceptable (Phase 8)
Each placement does slug lookup + insert individually. Acceptable at expected
scale. Batch optimization (WHERE slug = ANY($1)) deferred to Phase 8.

### Scored board tiebreaking uses entry_id (updated Phase 5)
When scores are equal, `ROW_NUMBER()` uses `entry_id ASC` as a tiebreaker for
deterministic position assignment. Originally used `placements.id` but this was
non-deterministic across versions (new UUID per insert). Changed to `entry_id`
which is stable across versions (assigned at entry creation).

### PlacementWithEntry doesn't include entry metadata (future enhancement)
Response includes entry slug and name only. Full entry metadata could be
added to the placement response in a future phase if needed.

### Entry deletion + version creation race is handled by PostgreSQL FK locking
When `entries::delete` holds FOR UPDATE on an entry, the FK constraint check
in `versions::create`'s placement INSERT implicitly acquires FOR KEY SHARE,
which conflicts with FOR UPDATE. This serializes the two operations correctly:
either the version creation waits and then fails with FK violation (if entry
was deleted) or the deletion sees the new placement and returns 409.

### Lost update race in board/entry PATCH is Phase 8 / v2
Concurrent PATCHes to the same board could overwrite each other's changes
(read-then-write pattern without locking). Acceptable for v1 given single-
curator semantics. Add SELECT FOR UPDATE in update functions if multi-admin
support is added.

### tier_config duplicate keys and COALESCE magic numbers are Phase 8
validate_tier_config doesn't enforce key uniqueness; fetch_placements uses
COALESCE with i32::MAX instead of NULLS LAST. Both are cosmetic/polish items.

### sort_direction on non-scored boards is harmless
Changing sort_direction on ordered/tiered boards succeeds silently. The field
has no effect on non-scored board logic. Phase 8 could reject or document.

## Phase 5

### Snapshot does not clear accumulated_scores (intentional)
Snapshot preserves all accumulated_scores — this is the "high score" pattern.
For resetting leaderboards (e.g., weekly standings), a "clear on snapshot" flag
or separate endpoint would be a future enhancement.

### No read endpoint for accumulated scores (design choice)
There is no `GET /boards/:slug/scores` to preview accumulated state before
snapshotting. The "snapshot materializes state" model is intentional. Could add
a preview endpoint in a future phase.

### Inline placement fetch in snapshot uses simpler query (intentional)
`scores::snapshot` fetches placements with a simpler ORDER BY (no LATERAL join
for tier ordering) instead of reusing `fetch_placements`. This is correct
because accumulative boards are always scored (never tiered). The simpler query
is appropriate for this context.

### Entry deletion cascade-deletes accumulated_scores (consistent)
If an entry has accumulated_scores but no snapshots, deleting the entry
cascade-deletes the score via FK. Consistent behavior — the entry has no
historical data in any version.

## Phase 6

### Sync bypasses API layer (intentional for v1)
The plan says sync should be an "API translator" but the implementation calls
repo functions directly. This is a pragmatic choice for v1: no auth exists yet
(Phase 7), no need for an HTTP client, no circular dependency issues, and no
requirement for the API server to be running during CLI sync. When auth and rate
limiting are added in Phases 7-8, sync operations will need to either go through
the API or have equivalent middleware applied.

### TOML parsing is permissive about unknown fields (forward-compatible)
`BoardToml` and `EntryToml` don't use `#[serde(deny_unknown_fields)]`. Typos
in field names are silently ignored. Trade-off: strict mode would break forward
compatibility. The permissive behavior is intentional.

### Sync is not atomic across boards (acceptable)
If syncing the second of three boards fails, the first board's changes are
already committed. The function logs failures and continues. This is a
reasonable tradeoff for simplicity — the plan does not specify transactional-
across-boards behavior.

### placements_changed only compares explicitly-set positions
For scored boards, TOML files omit position (None) but the DB stores derived
positions (Some(N)). The diff function only compares positions when the proposed
value explicitly sets one, preventing false change detection on every sync.

## Phase 7

### JWKS supports RSA keys only (scope limitation)
EC keys (ES256/ES384/ES512) and EdDSA are not supported. If the identity
provider uses EC keys, JWTs are rejected. Documented limitation for v1.

### validate_aud = false is intentional
JWT audience claims are not checked. For a single-purpose deployment (the
typical case), this is fine. Multi-tenant environments should add audience
validation in a future version.

### Auth token HMAC approach is sound
Uses HMAC-SHA256 with a fixed key purely for constant-time comparison via
`verify_slice()`. The fixed key is not a secret — it's used only to enable
the constant-time verification property.

### `since` endpoint returns version metadata only (no placements)
This is intentional — the endpoint is for staleness checks ("are there newer
versions?"), not for fetching full version data. Consumers should follow up
with a specific version endpoint if they need placements.

### `board_type`/`accumulative` immutability enforced by omission
`UpdateBoard` excludes these fields. A client sending them in PATCH gets no
error — the fields are silently ignored. This is consistent with the pattern
established in Phase 2.

### No pagination on list endpoints — Phase 8
All list endpoints return all records. Phase 8 will add cursor-based pagination.

### CORS middleware — Phase 8
`cors_origins` config field exists but no middleware is applied. Phase 8.

### `behind_proxy` config field — Phase 8
Parsed but unused. Intended for X-Forwarded-For handling in Phase 8.
