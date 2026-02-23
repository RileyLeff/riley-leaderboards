# riley_leaderboards v2 Plan

## What Changed Since v1

v1 delivered the full core: curated boards (ordered, scored, tiered), accumulative boards, versioning with diffs and history, file sync via git webhooks, auth (JWT + API token), export/import, pagination, rate limiting, and Docker deployment. 84 integration tests, 14 Docker smoke tests.

v2 builds on this foundation with six features that emerged from using v1:

1. **Version metadata** — attach arbitrary structured data to versions
2. **Read-only API keys** — separate read vs. write auth for embedding
3. **Outbound webhooks** — notify external services when boards change
4. **Board collections** — group related boards together
5. **Realtime boards** — Redis-backed high-throughput leaderboards
6. **Live updates via SSE** — push new standings to connected clients

---

## Feature 1: Version Metadata

### Problem

Versions have a `note` field (one-line text) but no way to attach structured data. Users want to link a blog post URL, a changelog, publication timestamps, or other context to each version update.

### Design

Add `metadata JSONB` to the `versions` table. Same pattern as boards, entries, and placements.

```sql
ALTER TABLE versions ADD COLUMN metadata jsonb;
```

**API changes:**
- `CreateVersion` gains optional `metadata` field
- `SnapshotInput` gains optional `metadata` field
- Version responses include `metadata`
- Export/import includes version metadata

**File sync changes:**
- `rankings.toml` gains an optional `[version_metadata]` section:
  ```toml
  [version_metadata]
  blog_post_url = "https://rileyleff.com/blog/sandwich-update"
  changelog = "Added Mangialardo's, moved Crunchy Boi to #1"
  ```

**No breaking changes.** Existing API calls work as before (metadata defaults to null).

---

## Feature 2: Read-Only API Keys

### Problem

v1 auth is binary: authenticated or not. Read endpoints are always public. For embedding leaderboards in frontends, users want to issue read-only keys that can be safely exposed in client-side code, while keeping write keys private. Some users also want to require auth for reads (private boards).

### Design

Replace the single `api_token` with a key system:

```toml
[auth]
# Admin key — full read/write access
admin_token = "env:LEADERBOARDS_ADMIN_TOKEN"

# Read-only keys — can fetch boards, versions, entries, references
# Multiple keys supported for different consumers
read_tokens = ["env:LEADERBOARDS_READ_TOKEN_1", "env:LEADERBOARDS_READ_TOKEN_2"]

# Whether reads require authentication. Default: false (public reads).
require_read_auth = false
```

**Auth middleware changes:**
- Write endpoints: require admin token, JWT with required role, or no-auth passthrough (unchanged behavior)
- Read endpoints: if `require_read_auth = true`, require any valid token (admin or read-only) or valid JWT
- If `require_read_auth = false` (default), reads are public (backwards-compatible)

**JWT mode:** JWTs can carry a `role` claim. The existing `required_role` applies to writes. Reads accept any valid JWT regardless of role (when `require_read_auth = true`).

**Backwards compatibility:** The existing `api_token` field continues to work as an alias for `admin_token`. Deployments that don't set `read_tokens` or `require_read_auth` behave exactly as v1.

---

## Feature 3: Outbound Webhooks

### Problem

When a board gets a new version, there's no way to notify external systems. The primary use case: trigger a static site rebuild (Netlify, Vercel, Cloudflare Pages) when curated rankings are updated via git sync.

### Design

New config section:

```toml
[[webhooks]]
url = "https://api.netlify.com/build_hooks/abc123"
events = ["version.created"]
# Optional: filter by board slug pattern
boards = ["dc-sandwiches", "nfl-*"]

[[webhooks]]
url = "https://api.vercel.com/v1/integrations/deploy/xyz"
events = ["version.created", "board.created"]
secret = "env:OUTBOUND_WEBHOOK_SECRET"
```

**Events:**
- `version.created` — a new version was created (any method: API, sync, snapshot)
- `board.created` — a new board was created
- `board.updated` — board metadata changed
- `board.deleted` — a board was deleted

**Payload format:**

```json
{
  "event": "version.created",
  "timestamp": "2026-02-23T12:00:00Z",
  "board": {
    "slug": "dc-sandwiches",
    "name": "Best Sandwiches in DC"
  },
  "version": {
    "version_number": 3,
    "note": "Added Mangialardo's"
  }
}
```

**Delivery:**
- Fire-and-forget: POST to URL with JSON payload, optional HMAC-SHA256 signature in `X-Webhook-Signature-256` header
- Retries: 3 attempts with exponential backoff (1s, 5s, 25s)
- Timeout: 10 seconds per attempt
- Runs asynchronously (tokio::spawn) — never blocks the API response

**No new database tables.** Webhook config lives entirely in the TOML file. No delivery tracking or retry persistence — this is a best-effort notification system, not a guaranteed delivery queue.

---

## Feature 4: Board Collections

### Problem

Users want to group related boards — "Riley's Food Rankings" contains the sandwich board, the pizza board, the ramen board. Useful for building index pages and navigation.

### Design

New database table:

```sql
CREATE TABLE collections (
  id uuid PRIMARY KEY DEFAULT uuidv7(),
  slug text UNIQUE NOT NULL,
  name text NOT NULL,
  metadata jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE collection_boards (
  collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  display_order integer NOT NULL DEFAULT 0,
  PRIMARY KEY (collection_id, board_id)
);
```

**API endpoints:**

| Method | Path | Description |
|--------|------|-------------|
| POST | `/collections` | Create a collection |
| GET | `/collections` | List collections (paginated) |
| GET | `/collections/:slug` | Get collection with its boards |
| PATCH | `/collections/:slug` | Update collection metadata |
| DELETE | `/collections/:slug` | Delete collection (not its boards) |
| POST | `/collections/:slug/boards` | Add a board to a collection |
| DELETE | `/collections/:slug/boards/:board_slug` | Remove a board from a collection |

**Response for `GET /collections/:slug`:**

```json
{
  "slug": "food-rankings",
  "name": "Riley's Food Rankings",
  "metadata": { "description": "All my DC food lists" },
  "boards": [
    { "slug": "dc-sandwiches", "name": "Best Sandwiches in DC", "latest_version": 3 },
    { "slug": "dc-pizza", "name": "Best Pizza in DC", "latest_version": 1 }
  ]
}
```

A board can belong to multiple collections. Deleting a collection does not delete its boards. Auth follows the same pattern: reads public, writes require admin auth.

**CLI commands:**
- `list-collections`
- `delete-collection <slug>`

---

## Feature 5: Realtime Boards (Redis-Backed)

### Problem

Accumulative boards write every score to Postgres. This works for moderate throughput (hundreds of writes/sec) but not for MMO-scale games with thousands of concurrent players submitting scores.

### Design

A new board type: `"realtime"`. Uses Redis as a hot tier for writes and reads, with periodic snapshots to Postgres for version history.

**Config:**

```toml
[redis]
url = "env:REDIS_URL"  # redis://localhost:6379
```

**How it works:**

1. **Score submission**: `POST /boards/:slug/scores` → `ZADD board:{slug}:scores {score} {entry_slug}` in Redis. Sub-millisecond writes. Entry metadata stored in a companion hash: `HSET board:{slug}:entries {entry_slug} {json}`.

2. **Current standings**: `GET /boards/:slug/latest` on a realtime board reads directly from Redis via `ZREVRANGE` (or `ZRANGE` for asc). Returns the same `VersionWithPlacements` response shape, but with `version_number: null` (it's live state, not a versioned snapshot).

3. **Snapshot**: `POST /boards/:slug/snapshot` reads the Redis sorted set, writes a Postgres version (same as accumulative boards). This creates a permanent versioned record. After snapshot, Redis state is preserved (high-score pattern) or cleared (reset pattern, controlled by a `clear_on_snapshot` board option).

4. **Fallback**: If Redis is unavailable, realtime boards return 503 for writes and reads. Snapshots still work if the last successful Redis state was captured. The system does not silently fall back to Postgres — that would change the performance characteristics without the operator knowing.

**Board creation:**

```json
{
  "slug": "forest-royale-live",
  "name": "Forest Royale Live Scores",
  "board_type": "scored",
  "accumulative": true,
  "realtime": true,
  "sort_direction": "desc"
}
```

`realtime` is a boolean flag on accumulative scored boards, not a separate board type. This way:
- A board can start as regular accumulative and upgrade to realtime later
- The version history, diff, since, and reference systems work identically
- Frontend code doesn't need to know whether the backend is Redis or Postgres
- If Redis is removed from the deployment, realtime boards gracefully degrade to regular accumulative

**What changes in the codebase:**
- New `redis` optional dependency in core crate (feature-flagged)
- `AppState` gains an `Option<redis::Client>` (None if no Redis configured)
- Score submission and latest-read have a Redis code path when `board.realtime && redis.is_some()`
- Snapshot reads from Redis instead of `accumulated_scores` table when realtime
- New config section `[redis]`

**What doesn't change:**
- All other board types (ordered, scored, tiered, accumulative-non-realtime)
- Version history, diff, since, references, export/import
- Auth, rate limiting, CORS, webhooks
- File sync (realtime boards are skipped, same as accumulative)

---

## Feature 6: Live Updates via SSE

### Problem

For game leaderboards (especially realtime boards), consumers want to see standings update in real-time without polling.

### Design

New endpoint: `GET /boards/:slug/stream`

Returns a Server-Sent Events stream. Events are pushed when:
- A new version is created (any board type)
- A score is submitted (realtime boards only — configurable)

**Event format:**

```
event: version.created
data: {"version_number": 5, "note": "Daily standings"}

event: score.updated
data: {"entry_slug": "rileyleff", "score": 1247, "position": 3}
```

**Implementation:**
- Uses tokio broadcast channels internally
- When a version is created or a score submitted, the handler publishes to the channel
- SSE connections subscribe to the channel for their board
- Connections are dropped after 30 minutes of inactivity (configurable)
- Rate limiting: score.updated events are debounced per-board (at most 1 event per second) to avoid flooding clients on high-throughput boards

**Config:**

```toml
[server]
sse_enabled = true                # default: false
sse_max_connections = 1000        # per-server limit
sse_score_debounce_ms = 1000     # minimum interval between score.updated events per board
```

**Auth:** SSE connections follow the same auth rules as read endpoints. If `require_read_auth = true`, the connection must include a valid token/JWT.

---

## Configuration (v2 additions)

Full v2 config showing new sections (existing v1 sections unchanged):

```toml
[server]
host = "0.0.0.0"
port = 8082
cors_origins = ["https://rileyleff.com"]
behind_proxy = false
rate_limit_per_second = 100
rate_limit_burst = 50
sse_enabled = false
sse_max_connections = 1000
sse_score_debounce_ms = 1000

[database]
url = "env:DATABASE_URL"
max_connections = 10
schema = "leaderboards"

[redis]
url = "env:REDIS_URL"

[auth]
jwks_url = "https://auth.rileyleff.com/.well-known/jwks.json"
required_role = "admin"
# Or token-based:
# admin_token = "env:LEADERBOARDS_ADMIN_TOKEN"
# read_tokens = ["env:LEADERBOARDS_READ_TOKEN"]
# require_read_auth = false

[sync]
repo_path = "/data/boards"
webhook_secret = "env:WEBHOOK_SECRET"

[[webhooks]]
url = "https://api.netlify.com/build_hooks/abc123"
events = ["version.created"]
boards = ["dc-*"]

[[webhooks]]
url = "https://discord.com/api/webhooks/xyz/abc"
events = ["version.created"]
secret = "env:DISCORD_WEBHOOK_SECRET"
```

---

## Database Schema Changes

### Migration 003: Version metadata + collections

```sql
-- Version metadata
ALTER TABLE versions ADD COLUMN metadata jsonb;

-- Collections
CREATE TABLE collections (
  id uuid PRIMARY KEY DEFAULT uuidv7(),
  slug text UNIQUE NOT NULL,
  name text NOT NULL,
  metadata jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE collection_boards (
  collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  display_order integer NOT NULL DEFAULT 0,
  PRIMARY KEY (collection_id, board_id)
);
```

### Migration 004: Realtime flag

```sql
ALTER TABLE boards ADD COLUMN realtime boolean NOT NULL DEFAULT false;

-- Realtime requires accumulative + scored
ALTER TABLE boards ADD CONSTRAINT realtime_requires_accumulative
  CHECK (NOT realtime OR (accumulative AND board_type = 'scored'));
```

---

## API Endpoints (v2 additions)

All v1 endpoints remain unchanged. New endpoints:

| Method | Path | Description |
|--------|------|-------------|
| POST | `/collections` | Create a collection |
| GET | `/collections` | List collections (paginated) |
| GET | `/collections/:slug` | Get collection with boards |
| PATCH | `/collections/:slug` | Update collection |
| DELETE | `/collections/:slug` | Delete collection |
| POST | `/collections/:slug/boards` | Add board to collection |
| DELETE | `/collections/:slug/boards/:board_slug` | Remove board from collection |
| GET | `/boards/:slug/stream` | SSE stream of board events |

---

## Implementation Phases

### Phase 1: Version Metadata
- Migration: add `metadata jsonb` to versions
- Update models: `Version`, `CreateVersion`, `SnapshotInput`
- Update API routes: version creation, snapshot, responses
- Update export/import to include version metadata
- Update file sync: parse `[version_metadata]` from rankings.toml
- Tests

### Phase 2: Read-Only API Keys
- Update config: `admin_token`, `read_tokens`, `require_read_auth`
- Backwards compat: `api_token` → `admin_token` alias
- Auth middleware: differentiate read vs. write access levels
- Tests (read-only token can read, can't write; admin can do both; public reads when not required)

### Phase 3: Outbound Webhooks
- Config parsing for `[[webhooks]]`
- Webhook dispatcher: async HTTP POST with HMAC signing, retries, timeout
- Hook into version creation, board CRUD to fire events
- Board slug pattern matching for filtered webhooks
- Tests

### Phase 4: Board Collections
- Migration: collections + collection_boards tables
- Collection CRUD (repo + API routes)
- Board membership management
- CLI commands: list-collections, delete-collection
- Tests

### Phase 5: Realtime Boards (Redis)
- Redis config + optional connection in AppState
- Feature flag: `realtime` on boards, constraint enforcement
- Score submission via Redis ZADD
- Latest read via Redis ZRANGE
- Snapshot from Redis → Postgres version
- `clear_on_snapshot` option
- Fallback behavior (503 when Redis unavailable)
- Tests (with Redis in Docker test infrastructure)

### Phase 6: Live Updates (SSE)
- Broadcast channel infrastructure
- SSE endpoint with auth
- Publish events from version creation + score submission
- Debouncing for score events
- Connection limits + timeout
- Config options
- Tests

---

## What's NOT in v2

- Voting / user-submitted rankings with per-user auth — v3
- Board collaboration (multiple curators with conflict resolution) — v3
- Computed/formula boards (rankings derived from other boards) — v3
- Admin web UI — v3
- Image/thumbnail generation for social sharing — v3
- RSS feeds for board updates — v3
- Entry merge/split operations — v3
- GraphQL API — v3
