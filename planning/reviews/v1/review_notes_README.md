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
