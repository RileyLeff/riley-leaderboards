# Fixes for Review Round 12 — Phase 4 R1

**Commit:** 926d7be

## Fixed

1. **[consensus] CLI ListCollections pagination** — Added `collections::list()` non-paginated function (mirroring `boards::list()`), replaced `list_paginated` with hard-coded limit in CLI.

2. **[codex-only] FK race in add_board** — Mapped `is_foreign_key_violation()` to `Error::NotFound` in the `add_board` error handler, so concurrent deletes produce 404 instead of 500.

3. **[codex-only] Missing board_id index** — Added `CREATE INDEX idx_collection_boards_board_id ON collection_boards(board_id)` to migration 004.

4. **[claude-only] No-op PATCH guard** — Added early return in `collections::update()` when `name.is_none() && metadata.is_absent()`, skipping the UPDATE and preserving `updated_at`.

## Deferred

5. **[codex-only] Missing auth integration tests** — Will address in R2 if needed; existing auth middleware is shared with boards and well-tested there.

6. **[claude-only] Missing entry_count in CollectionBoardEntry** — Nice-to-have, not in the plan. Deferred.

7. **[claude-only] No collection webhook events** — Intentional per plan; collections are organizational, not data-bearing.

8. **[claude-only] Correlated subquery in get_with_boards** — Fine at current scale; index-only scan on versions PK.
