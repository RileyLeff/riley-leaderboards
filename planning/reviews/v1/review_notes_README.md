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

### Scored board tiebreaking uses placement ID
When scores are equal, `ROW_NUMBER()` uses `id ASC` as a tiebreaker for
deterministic position assignment across transactions.

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
