# Round 1 Fixes — 2026-02-22

## Major

### 1. Schema creation race condition
**Fixed in** `52f0dc5`

Created schema via a one-off bootstrap connection before building the pool.
Added `connect_readonly()` for the `validate` command so it doesn't create
schemas as a side effect.

## Minor

### 2. Redundant indexes
**Fixed in** `07d8338`

Removed 4 redundant indexes that duplicated UNIQUE constraint B-tree indexes.

### 3. CHECK constraints
**Fixed in** `07d8338`

Added CHECK constraints on `board_type`, `sort_direction`, `ref_type`, and
`version_number > 0`.

### 4. Health check test
**Fixed in** `fb55174`

Added integration test for `/health` using tower oneshot.

### 5. ConfigValue simplification
**Fixed in** `bc41b40`

Replaced single-variant enum with `#[serde(transparent)]` newtype struct.

### 6. Error chain preservation
**Fixed in** `52f0dc5`

Changed `anyhow::anyhow!("{e}")` to `anyhow::anyhow!(e)` to preserve the
error chain.

### 7. validate command side effects
**Fixed in** `52f0dc5`

`validate` now uses `connect_readonly()` which does not create schemas.

### 8. SIGTERM handling
**Fixed in** `fb55174`

`shutdown_signal()` now listens for both SIGINT and SIGTERM via `tokio::select!`.

### 9. Test cleanup error handling
**Fixed in** `bc41b40`

Changed `.ok()` to `.expect()` on DROP SCHEMA in test cleanup.

## Not Fixed (Deferred)

- **Health check cast** (finding #2): Fixed — explicit `::int4` cast added.
- **Migration prefix convention** (finding #12): Keeping integer prefix — solo
  project, no collision risk, consistent with existing files.
- **home_dir Unix-only** (finding #11): Keeping `$HOME` — targets Linux/macOS
  deployment per plan.
