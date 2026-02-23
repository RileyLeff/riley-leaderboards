# Review Round 1 — 2026-02-22

**Models**: Codex, Gemini, Claude
**Context**: ~17k tokens
**Phase**: Phase 1 Foundation (exhaustive review)

## Findings

### Major

#### 1. Schema creation race condition in `db::connect()` [consensus: Claude + Codex]

**Files:** `crates/riley-leaderboards-core/src/db.rs:24-54`

The `after_connect` hook sets `search_path` on every new connection, but the pool may eagerly open connections before `CREATE SCHEMA IF NOT EXISTS` runs. If someone adds startup logic that queries the pool between `.connect()` and the CREATE SCHEMA call, it breaks silently.

- **Claude**: Calls this major — the invariant is not enforced and will break when the code grows. Recommends creating the schema via a separate one-off connection *before* constructing the pool.
- **Codex**: Does not flag this directly.
- **Gemini**: Notes the order of operations is safe *today* because Postgres allows setting `search_path` to a non-existent schema without error. But agrees the window exists.

**Verdict: major** — safe today by coincidence, not by design. Fix by creating schema before building the pool.

---

### Minor

#### 2. Redundant indexes duplicate UNIQUE constraint indexes [consensus: Codex + Claude]

**Files:** `migrations/001_initial_schema.sql`

Several explicit single-column indexes are redundant because they duplicate the leading column of existing UNIQUE constraints (Postgres uses leftmost prefix matching on B-tree indexes):
- `idx_entries_board_id` — redundant with `UNIQUE (board_id, slug)`
- `idx_accumulated_scores_board_id` — redundant with `UNIQUE (board_id, entry_id)`
- `idx_versions_board_id_number` — redundant with `UNIQUE (board_id, version_number)`
- `idx_placements_version_id` — redundant with composite on `(version_id, ...)`

**Verdict: minor** — remove redundant indexes to save space and write overhead.

#### 3. No CHECK constraints on enum-like text columns [consensus: Codex + Claude]

**Files:** `migrations/001_initial_schema.sql`

`board_type`, `sort_direction`, and `ref_type` allow any string value. Add CHECK constraints:
```sql
board_type text NOT NULL CHECK (board_type IN ('ordered', 'scored', 'tiered'))
sort_direction text NOT NULL DEFAULT 'desc' CHECK (sort_direction IN ('asc', 'desc'))
ref_type text NOT NULL CHECK (ref_type IN ('embed', 'citation', 'context'))
```

**Verdict: minor** — defense-in-depth at the DB layer.

#### 4. Missing health check endpoint test [consensus: Codex + Gemini]

**Files:** `crates/riley-leaderboards-api/src/lib.rs`

Plan calls for health endpoint testing. No unit/integration test for the health handler exists.

**Verdict: minor** — rounds out Phase 1 verification.

#### 5. `ConfigValue` enum design [consensus: Gemini + Claude]

**Files:** `crates/riley-leaderboards-core/src/config.rs:56`

Single-variant enum with `#[serde(untagged)]` is misleading. Gemini suggests renaming `Literal` to `Raw` or `Unresolved`. Claude suggests either adding a second variant or using a plain String newtype.

**Verdict: minor** — unnecessary abstraction for a single variant.

#### 6. Error chain lost through stringification [claude-only]

**Files:** `crates/riley-leaderboards-cli/src/main.rs`

`anyhow::anyhow!("{e}")` stringifies the error, losing the chain. Since core's Error implements `std::error::Error`, `?` should work directly.

**Verdict: minor** — makes debugging harder.

#### 7. `validate` command creates schemas as a side effect [codex-only]

**Files:** `crates/riley-leaderboards-cli/src/main.rs`

The `validate` command calls `db::connect()`, which runs `CREATE SCHEMA IF NOT EXISTS`. A read-only validation command shouldn't mutate DB state.

**Verdict: minor** — validate should be non-destructive.

#### 8. No SIGTERM handling for graceful shutdown [consensus: Codex + Claude]

**Files:** `crates/riley-leaderboards-api/src/lib.rs`

`shutdown_signal` only listens for Ctrl+C (SIGINT). Docker sends SIGTERM. Without handling it, containers wait 10s then get SIGKILL.

**Verdict: minor** — needed before Docker deployment (Phase 9), but fixing now is cheap.

#### 9. Integration test cleanup swallows errors [claude-only]

**Files:** `crates/riley-leaderboards-core/tests/db_integration.rs`

`.ok()` on DROP SCHEMA silences all errors including connection/permission failures. Should log or expect.

**Verdict: minor** — test hygiene.

---

### Notes

#### 10. Auto-migrate on serve is convenient but opinionated [claude-only]
`serve` auto-runs migrations. Fine for v1, but should eventually be configurable.

#### 11. `home_dir()` only checks `$HOME` [claude-only]
Correct for Unix/Linux target. Only matters if cross-platform is a goal.

#### 12. Migration uses integer prefix, not timestamp [claude-only]
`001_` works but timestamp prefixes are conventional for multi-contributor projects.

#### 13. Unnecessary ServerConfig clone [claude-only]
Could use a reference instead. Negligible performance impact.

#### 14. No `updated_at` trigger [claude-only]
Column will go stale without application-level SET or a DB trigger. Track for Phase 2.

#### 15. No uniqueness constraint on `board_references.uri` [claude-only]
Design decision to clarify: can the same URI reference a board twice?

#### 16. `placements.score` uses `double precision` [claude-only]
`numeric` would be exact but slower. Acceptable tradeoff for game scores.

#### 17. No `CHECK (version_number > 0)` [claude-only]
Application logic will enforce in Phase 2, but DB constraint is defense-in-depth.

#### 18. No test for public schema migration path [claude-only]
`migrate_default_schema` actually uses a custom schema. Understandable for test isolation.

#### 19. Crate boundaries are clean and match the plan [consensus: all three]
Positive observation from all models.

#### 20. Soul document and plan are well-aligned with implementation [consensus: Gemini + Claude]
Positive observation.

#### 21. `quote_identifier` SQL injection prevention is correct [consensus: Gemini + Claude]
The double-quote escaping pattern is the standard Postgres identifier quoting approach.

#### 22. Schema SQL and Postgres 18 features are well-used [consensus: Gemini + Claude]
UUIDv7, reserved keyword avoidance, cascading deletes all done correctly.
