# Phase 8 Exhaustive Review R1 — Fixes

**Commit**: bbd426a

## Major Fixes

### 1. Import not transactional (was Major 1)
Wrapped entire `import_board` in `pool.begin()` / `tx.commit()`. All SQL operations use `&mut *tx`. If any step fails, the transaction rolls back automatically.

### 2. Import bypasses placement validation (was Major 2)
Added `validate_placements_for_import()` public wrapper in `versions.rs` that delegates to the existing private `validate_placements()`. Import now pre-validates all version placements before starting the transaction, using a temporary Board struct.

### 3. Import doesn't derive scored positions (was Major 3)
After inserting placements for each version, `import_board` now calls `derive_scored_positions()` for scored boards, matching what `create_version` does.

### 4. Webhook route registered without sync config (was Major 4)
Made webhook route conditional: only registered when `state.config.sync.is_some()`. Previously the route was always present and would return 500 if sync wasn't configured.

## Minor Fixes

### 5. Import uses first-seen entry name (was Minor 6)
Changed entry name collection to use `HashMap::insert` across all versions, so the last-seen name wins. Previously collected with `entry()` API which kept first-seen.

### 6. Rate limiting on CORS preflight / layer ordering (was Minors 10/11)
Reordered Axum middleware layers so CORS is outermost (applied last = executed first). This ensures preflight OPTIONS requests get CORS headers without hitting rate limiting first.

### 7. Webhook git pull doesn't specify branch (was Minor 14)
Added `"origin", expected_branch` to the `git pull` args so it pulls the expected branch explicitly rather than relying on default tracking.

## Not Fixed (Intentional / Deferred)

See `review_notes_README.md` for notes on items recorded as intentional design decisions (pagination max limit, configurable rate limiting, etc.).
