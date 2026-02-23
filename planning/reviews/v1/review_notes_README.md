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
