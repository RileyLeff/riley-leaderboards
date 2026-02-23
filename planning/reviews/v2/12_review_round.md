# Review Round 12 — Phase 4 Exhaustive R1 (2026-02-23)

**Models**: Codex, Claude (Gemini unavailable)
**Context**: ~182k tokens
**Scope**: Full codebase — Phase 4 Board Collections

## Findings

### Major

None.

### Minor

1. **[consensus] CLI `ListCollections` limited to 200 items with no pagination loop** — `main.rs:Command::ListCollections` constructs `PaginationParams { limit: Some(200), cursor: None }` and prints only the first page. `ListBoards` uses a non-paginated `list()`, so this is inconsistent. Should either loop to exhaustion or add a `list_all()`.

2. **[codex-only] Concurrent delete race in `add_board` can return 500 instead of 404** — `repo/collections.rs:add_board()` looks up collection and board separately, then inserts. If either is deleted between lookup and insert, FK violation maps to `Error::Database` (500) rather than `NotFound` (404). Should map FK-violation errors to NotFound.

3. **[codex-only] Missing index on `collection_boards.board_id`** — PK is `(collection_id, board_id)` so board-driven operations (including cascade deletes) don't get a leading-column index on `board_id`. Should add `CREATE INDEX idx_collection_boards_board_id ON collection_boards(board_id)`.

4. **[codex-only] Missing auth integration tests for collection endpoints** — Board auth tests exist but no equivalent tests proving `/collections` writes are blocked when auth is configured, or that reads follow `require_read_auth` behavior.

5. **[claude-only] Collection PATCH with no changes still writes to database** — `repo/collections.rs:update()` always executes UPDATE even when body is `{}`, which bumps `updated_at`. Should skip the write when nothing changed.

6. **[claude-only] `CollectionBoardEntry` missing `entry_count` field** — The `GET /boards/:slug` response includes `entry_count` but `CollectionBoardEntry` in collection detail does not. Consumers building index pages would need extra API calls.

### Notes

7. Auth middleware correctly applied to all collection routes — reads public, writes require admin auth. Verified in `lib.rs:collection_routes()`.
8. Cascading deletes are bidirectional and tested — delete collection removes memberships, delete board removes from collections.
9. Slug/name validation reuses shared validators from `repo/mod.rs`.
10. Nullable PATCH semantics correctly implemented for metadata.
11. Composite PK prevents duplicate memberships; repo correctly maps to 409.
12. `display_order` defaults to 0 with alphabetical fallback sort — reasonable design.
13. No regressions from prior phases detected.
14. [claude-only] No outbound webhook events for collection CRUD — likely intentional since collections are organizational, not data-bearing.
15. [claude-only] `get_with_boards` correlated subquery for `latest_version` is fine at current scale (index-only scan on versions PK).
