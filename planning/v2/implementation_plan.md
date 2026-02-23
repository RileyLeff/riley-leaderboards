# v2 Implementation Plan

Derived from `planning/v2/plan.md`. Each phase maps to a v2 feature. Steps within a phase are atomic units of work, each resulting in one or more commits.

---

## Phase 1: Version Metadata

**Goal:** Versions can carry arbitrary structured data (blog post URLs, changelogs, etc.).

- **1.1** Migration — `ALTER TABLE versions ADD COLUMN metadata jsonb`
- **1.2** Model updates — add `metadata` to `Version`, `CreateVersion`, `SnapshotInput`, `VersionExport`
- **1.3** API + repo wiring — version creation, snapshot, and responses include metadata
- **1.4** Export/import — round-trip version metadata through export/import
- **1.5** File sync — parse optional `[version_metadata]` from `rankings.toml`, pass through to version creation
- **1.6** Tests — version creation with metadata, snapshot with metadata, export/import round-trip, sync with version_metadata

**Review:** Standard review after Phase 1.

---

## Phase 2: Read-Only API Keys

**Goal:** Separate read vs. write auth for safe embedding in frontends.

- **2.1** Config changes — `admin_token`, `read_tokens`, `require_read_auth`, backwards-compat alias for `api_token`
- **2.2** Auth middleware refactor — pass access level (read/write) to middleware, check token type against required level
- **2.3** Tests — read-only token reads but can't write, admin token does both, public reads when `require_read_auth = false`, backwards compat with `api_token`

**Review:** Standard review after Phase 2.

---

## Phase 3: Outbound Webhooks

**Goal:** External services are notified when boards change.

- **3.1** Config parsing — `[[webhooks]]` array with url, events, boards, secret
- **3.2** Webhook dispatcher — async HTTP POST, HMAC-SHA256 signing, 3 retries with backoff, 10s timeout
- **3.3** Event hooks — fire `version.created` from version creation (API, sync, snapshot), `board.created/updated/deleted` from board CRUD
- **3.4** Board filtering — glob pattern matching on board slugs
- **3.5** Tests — webhook fires on version creation, HMAC signature verification, board pattern filtering, retry behavior

**Review:** Standard review after Phase 3.

---

## Phase 4: Board Collections

**Goal:** Group related boards for index pages and navigation.

- **4.1** Migration — collections + collection_boards tables
- **4.2** Collection CRUD — repo layer (create, get, list, update, delete)
- **4.3** Board membership — add/remove boards from collections, display_order
- **4.4** API routes — all collection endpoints wired up
- **4.5** CLI commands — `list-collections`, `delete-collection`
- **4.6** Tests — CRUD, board membership, pagination, cascading deletion

**Review:** Exhaustive review after Phase 4 (all Postgres-only features complete).

---

## Phase 5: Realtime Boards (Redis)

**Goal:** Redis-backed high-throughput leaderboards with the same API shape.

- **5.1** Redis config + connection — `[redis]` config section, `Option<redis::Client>` in AppState
- **5.2** Board model changes — `realtime` boolean flag, DB constraint (requires accumulative + scored)
- **5.3** Score submission via Redis — ZADD for realtime boards, entry metadata in Redis hash
- **5.4** Latest read via Redis — ZREVRANGE/ZRANGE returning `VersionWithPlacements` shape
- **5.5** Snapshot from Redis — read sorted set, create Postgres version, optional `clear_on_snapshot`
- **5.6** Fallback behavior — 503 when Redis unavailable for realtime boards
- **5.7** Tests — score submission, latest read, snapshot, clear_on_snapshot, Redis unavailable, upgrade from regular accumulative

**Review:** Exhaustive review after Phase 5 (new infrastructure component).

---

## Phase 6: Live Updates (SSE)

**Goal:** Real-time push notifications for board changes.

- **6.1** Broadcast infrastructure — per-board tokio broadcast channels, managed by a registry
- **6.2** SSE endpoint — `GET /boards/:slug/stream` with auth, connection limits, timeout
- **6.3** Event publishing — publish to channels from version creation + score submission
- **6.4** Debouncing — rate-limit score.updated events per board
- **6.5** Config — `sse_enabled`, `sse_max_connections`, `sse_score_debounce_ms`
- **6.6** Tests — SSE connection, event delivery on version creation, debouncing, auth enforcement, connection limit

**Review:** Exhaustive review (final milestone).

---
