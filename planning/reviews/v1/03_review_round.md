# Review Round 2 — 2026-02-22

**Models**: Gemini, Claude (Codex still running, dropped from round)
**Context**: ~42k tokens
**Phase**: Phase 1 Foundation (exhaustive review, round 2 of 2)

## Fix Verification

All 9 round 1 fixes verified as correctly implemented by both Claude and Gemini:
- Schema creation race condition: bootstrap connection before pool
- Redundant indexes removed
- CHECK constraints added
- ConfigValue simplified to newtype
- Error chain preservation
- validate uses connect_readonly
- SIGTERM handling
- Test cleanup uses .expect()
- Health endpoint test added

## Findings

### Major

None.

**Note on Gemini false positive**: Gemini flagged `sqlx::migrate!("../../migrations")` as incorrect, claiming the path resolves wrong. This is a false positive — `sqlx::migrate!` resolves paths relative to `CARGO_MANIFEST_DIR` (i.e., `crates/riley-leaderboards-core/`), so `../../migrations` correctly reaches the workspace root. The compile-time macro verification and passing tests confirm this. Gemini itself verified this was correct in round 1.

### Minor

#### 1. ref_type CHECK constraint values diverge from plan [consensus: Claude + Gemini]

**Files:** `migrations/001_initial_schema.sql`, `planning/v1/plan.md`

The migration uses `('embed', 'citation', 'context')` but the plan uses `('blog_post', 'game', 'page')`. Both models agree the implementation's taxonomy is stronger (describes relationship nature vs. entity type). Update the plan to match.

#### 2. Validate command error stringification [gemini-only]

**Files:** `crates/riley-leaderboards-cli/src/main.rs:66`

The validate command uses `.map_err(|e| anyhow::anyhow!("database connection failed: {e}"))` which stringifies the error. Should use `.context("database connection failed")?` instead.

### Notes

#### 3. Unix-specific signal code not cfg-gated [gemini-only]
`shutdown_signal()` uses `tokio::signal::unix::signal` without `#[cfg(unix)]`. Project targets Linux/macOS so this is acceptable. Note for future if cross-platform is desired.

#### 4. Unnecessary .map_err() on load_config [claude-only]
The `?` operator handles the conversion automatically since `core::error::Error` implements `std::error::Error + Send + Sync + 'static`.

#### 5. Integration tests leak schemas on assertion panic [claude-only]
If an assertion panics before cleanup, schemas are left behind. Harmless in dev DB but worth noting.

#### 6. connect_readonly sets search_path to potentially non-existent schema [claude-only]
PostgreSQL silently accepts this. Validate confirms connectivity but not schema existence. Fine for current intent.

#### 7. ConfigValue "env:" prefix is implicit [gemini-only]
A password starting with "env:" would be misinterpreted. Acceptable for internal tooling, document for users.

#### 8. All round 1 fixes verified correct [consensus: Claude + Gemini]
Positive observation.

## Summary

| Severity | Count |
|----------|-------|
| Major | 0 |
| Minor | 2 |
| Notes | 6 |

**Consecutive rounds with zero major: 1** (need 2 for convergence)
