# riley_leaderboards v1 Plan

## Philosophy

riley_leaderboards is a **general-purpose versioned ranking service**. It is not specific to rileyleff.com. Another developer should be able to deploy it for their own site, games, or apps. Site-specific rendering, styling, and interaction design live in the consuming frontend — the service provides data, versioning, and cross-references.

The core insight: **rankings change over time, and that history is valuable.** A sandwich list published in January looks different by March. A game leaderboard shifts every day. riley_leaderboards treats change as a first-class concept — every state is a version, versions are immutable, and external systems can pin to specific versions or follow the latest.

The same ethos as riley_auth and riley_cms: minimal, self-hosted, stateless-friendly, bring-your-own-Postgres, configurable for any deployment topology.

---

## Architecture

### The Stack

```
Git repo (TOML files)
    ↓ push
GitHub webhook
    ↓ triggers
CLI sync
    ↓ calls
API  ←──── game servers, frontends, other services
    ↓
Postgres
```

Every layer calls the one below it. There is **one code path** for creating versions — the API. Everything above it is a convenience wrapper:

- **API** — the only thing that talks to the database. All reads and writes go through here. This is the bottom of the stack and the single source of truth.
- **CLI `sync`** — reads TOML files from a directory, diffs against current state via the API, creates/updates boards and versions via the API. It's a file-to-API translator. Does not touch the DB directly.
- **Webhook endpoint** — receives a GitHub push event, pulls the repo to a local directory, runs the same sync logic as the CLI. It's a git-event-to-CLI trigger.

Each layer is optional. A game server talks directly to the API. A curator who doesn't want git can curl the API. The git/sync/webhook path is optimized for the "human curating a list" use case.

### Two Input Patterns

**Curated boards** (sandwiches, tier lists, draft prospects): Files in a git repo → push → sync → API → new version. Human-driven, infrequent updates, every version is intentional.

**Accumulative boards** (game scores): Game server → API → score accumulates → snapshot creates version. Machine-driven, frequent updates, versions are periodic snapshots.

Both produce the same data — versions with placements. A consumer fetching `/boards/:slug/versions/:v` gets the same shape regardless of how the version was created.

---

## Crate Structure

```
riley_leaderboards/
├── Cargo.toml                              # workspace root
├── riley_leaderboards.example.toml         # example config
├── migrations/                             # sqlx migrations
├── Dockerfile
└── crates/
    ├── riley-leaderboards-core/            # library: config, db, models, versioning, sync logic
    ├── riley-leaderboards-api/             # HTTP server: routes, middleware, webhook handler
    └── riley-leaderboards-cli/             # binary: serve, migrate, sync, admin commands
```

---

## Core Concepts

### Board

A ranked list. Examples: "Best DC Sandwiches", "Forest Royale High Scores", "NBA Draft Prospects 2026". A board has a slug, display name, a type that determines how entries are ranked, and arbitrary metadata.

Board types:

| Type | How ranking works | Example |
|------|-------------------|---------|
| `ordered` | Entries have explicit positions set by a curator | Best sandwiches, book recommendations |
| `scored` | Entries have numeric scores, position is derived (highest first by default) | Game high scores, ratings |
| `tiered` | Entries are grouped into named tiers, optionally ordered within tiers | Tier lists (S/A/B/C/D), recommendation categories |

### Entry

A persistent entity on a board. Has a stable ID and slug that survives across versions. Examples: "Bub & Pop's", "player:rileyleff", "Zacch Pickens".

Entries belong to the **board**, not to individual versions. This is critical — it enables tracking how an entry moves across versions (the NBA draft prospect animation use case, the sandwich that climbs the rankings over time).

Entries carry arbitrary metadata as JSONB (image URL, link, address, player stats — whatever the consumer needs). Metadata can vary per entry and is not schema-enforced.

### Version

An immutable snapshot of a board's rankings at a point in time. Every edit to rankings creates a new version. Versions have a sequential number, a timestamp, and an optional note ("Added Bub & Pop's", "March 2026 update", "Post-tournament standings").

Versions are the key primitive. They enable:
- Slider UX (scrub through history)
- "This has been updated since publication" indicators
- Diffing between any two points in time
- Entry movement tracking across time

### Placement

The join between an entry and a version. A placement says "in version 5, entry X was at position 3 with score 847 in tier S." What fields are populated depends on the board type:

| Field | `ordered` | `scored` | `tiered` |
|-------|-----------|----------|----------|
| `position` | required (explicit) | derived (by score) | optional (within-tier ordering) |
| `score` | null | required | null |
| `tier` | null | null | required |
| `metadata` | optional (jsonb) | optional (jsonb) | optional (jsonb) |

Placement metadata is version-specific context about an entry — "was injured this week", "new menu item", etc. Distinct from entry-level metadata which is stable identity info.

### Reference

A link between a board (at a specific version) and some external context. This is the "awareness" layer that enables "this has been updated since you last saw it."

Examples:
- Blog post published embedding version 3 of "DC Sandwiches" → `{ board: "dc-sandwiches", pinned_version: 3, uri: "/blog/sandwich-rankings", type: "blog_post" }`
- Forest Royale always shows latest scores → `{ board: "forest-royale-scores", pinned_version: null, uri: "https://forestroyale.rileyleff.com", type: "game" }`
- A follow-up blog post references a newer version → `{ board: "dc-sandwiches", pinned_version: 7, uri: "/blog/sandwich-update-march", type: "blog_post" }`

The API can answer: "given board X pinned at version 3, what's the latest version?" and "what other references exist for this board?" — that's everything a frontend needs to render staleness indicators, update links, and version sliders.

References are optional. A board works fine with zero references.

---

## File Format (for git-based curated boards)

Board definitions live in a git repo as TOML files. The sync layer reads these and translates them into API calls.

### Directory structure

```
boards/
├── dc-sandwiches/
│   ├── board.toml
│   └── rankings.toml
├── nfl-draft-2026/
│   ├── board.toml
│   └── rankings.toml
└── best-programming-languages/
    ├── board.toml
    └── rankings.toml
```

Each board is a directory named by its slug. Two files per board.

### board.toml — board configuration

```toml
name = "Best Sandwiches in DC"
board_type = "ordered"

[metadata]
description = "A definitive and correct ranking of DC sandwiches."
```

Tiered board example:

```toml
name = "2026 NFL Draft Prospect Rankings"
board_type = "tiered"

[[tiers]]
key = "elite"
label = "Elite (Top 5 Pick)"

[[tiers]]
key = "first_round"
label = "First Round"

[[tiers]]
key = "second_round"
label = "Day 2"

[[tiers]]
key = "sleeper"
label = "Sleeper"
```

Scored board example:

```toml
name = "Forest Royale High Scores"
board_type = "scored"
accumulative = true
sort_direction = "desc"
```

### rankings.toml — current state of entries and placements

Ordered board:

```toml
[[entries]]
slug = "crunchy-boi"
name = "Compliments Only Crunchy Boi"
position = 1

[entries.metadata]
address = "1026 Vermont Ave NW"
image_url = "https://assets.rileyleff.com/sandwiches/crunchy-boi.jpg"

[[entries]]
slug = "humberto"
name = "Dupont Market Humberto"
position = 2

[entries.metadata]
address = "1807 18th St NW"

[[entries]]
slug = "a-litteri"
name = "A. Litteri Italian"
position = 3
```

Tiered board:

```toml
[[entries]]
slug = "travis-hunter"
name = "Travis Hunter"
tier = "elite"
position = 1

[[entries]]
slug = "cam-ward"
name = "Cam Ward"
tier = "elite"
position = 2

[[entries]]
slug = "tetairoa-mcmillan"
name = "Tetairoa McMillan"
tier = "first_round"
position = 1
```

Position within the `[[entries]]` array can also imply order — if `position` is omitted, array order is used. Explicit positions override array order.

### How sync works

1. `riley-leaderboards sync /path/to/boards/` (or triggered by webhook)
2. For each board directory:
   a. Read `board.toml` — create or update the board via `POST /boards` or `PATCH /boards/:slug`
   b. Read `rankings.toml` — parse entries and placements
   c. Fetch current state from API: `GET /boards/:slug/latest`
   d. Diff: compare parsed placements against current version
   e. If nothing changed, skip
   f. If changed: create any new entries via `POST /boards/:slug/entries`, then create a new version via `POST /boards/:slug/versions` with all placements
3. Version note comes from `--note` CLI flag, or falls back to a default ("Synced from file")

When triggered by webhook, the note can be extracted from the git commit message in the push payload.

### What sync does NOT do

- **Delete boards** that are missing from the directory. This prevents accidental data loss. Board deletion is an explicit action (`riley-leaderboards delete-board <slug>` or `DELETE /boards/:slug`).
- **Delete entries** that are missing from `rankings.toml`. Entries that were in previous versions but aren't in the current file simply don't get placements in the new version. They remain in the database for history queries.
- **Touch accumulative boards**. If a board has `accumulative = true`, sync skips it — those are managed via score submission API, not files.

---

## Configuration (riley_leaderboards.toml)

Same resolution order as riley_auth and riley_cms: CLI flag > env var > cwd > walk up > ~/.config > /etc.

Values support `"env:VAR_NAME"` syntax for secrets.

```toml
[server]
host = "0.0.0.0"
port = 8082
cors_origins = ["https://rileyleff.com", "https://*.rileyleff.com"]
behind_proxy = false

[database]
url = "env:DATABASE_URL"               # postgres://user:pass@host/db
max_connections = 10
schema = "leaderboards"                # optional, defaults to "public"

[auth]
# Optional: if set, write operations require a valid JWT from riley_auth.
# If unset, write operations are open (or use api_token below).
jwks_url = "https://auth.rileyleff.com/.well-known/jwks.json"
required_role = "admin"                # role claim required for write operations

# Alternative: simple shared-secret auth for environments without riley_auth.
# Mutually exclusive with jwks_url.
# api_token = "env:LEADERBOARDS_API_TOKEN"

[sync]
repo_path = "/data/boards"             # local path where board files are checked out
webhook_secret = "env:WEBHOOK_SECRET"  # HMAC-SHA256 secret for verifying GitHub webhooks

[boards]
max_entries_per_version = 1000         # safety limit
max_versions_per_board = 10000         # safety limit
max_metadata_size_bytes = 65536        # 64KB per JSONB field
```

---

## Database Schema (Postgres 18)

### boards

```sql
CREATE TABLE boards (
  id uuid PRIMARY KEY DEFAULT uuidv7(),
  slug text UNIQUE NOT NULL,
  name text NOT NULL,
  board_type text NOT NULL,             -- 'ordered', 'scored', 'tiered'
  sort_direction text NOT NULL DEFAULT 'desc',  -- 'asc' or 'desc' (for scored boards)
  tier_config jsonb,                    -- tier labels, ordering, display hints (tiered boards only)
  metadata jsonb,                       -- description, rules, display hints
  accumulative boolean NOT NULL DEFAULT false,   -- true = scores stream in, false = curated
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);
```

`tier_config` example for a tiered board:
```json
{
  "tiers": [
    { "key": "S", "label": "S Tier", "position": 1 },
    { "key": "A", "label": "A Tier", "position": 2 },
    { "key": "B", "label": "B Tier", "position": 3 },
    { "key": "C", "label": "C Tier", "position": 4 },
    { "key": "D", "label": "D Tier", "position": 5 }
  ]
}
```

Or custom tiers:
```json
{
  "tiers": [
    { "key": "must_try", "label": "Must Try", "position": 1 },
    { "key": "solid", "label": "Solid Choice", "position": 2 },
    { "key": "skip", "label": "Skip It", "position": 3 }
  ]
}
```

### entries

```sql
CREATE TABLE entries (
  id uuid PRIMARY KEY DEFAULT uuidv7(),
  board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  slug text NOT NULL,
  name text NOT NULL,
  metadata jsonb,                       -- stable identity info (image, link, etc.)
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (board_id, slug)
);

CREATE INDEX idx_entries_board_id ON entries(board_id);
```

### versions

```sql
CREATE TABLE versions (
  id uuid PRIMARY KEY DEFAULT uuidv7(),
  board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  version_number integer NOT NULL,
  note text,                            -- optional: "Added Bub & Pop's", "March update"
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (board_id, version_number)
);

CREATE INDEX idx_versions_board_id_number ON versions(board_id, version_number);
```

### placements

```sql
CREATE TABLE placements (
  id uuid PRIMARY KEY DEFAULT uuidv7(),
  version_id uuid NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
  entry_id uuid NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
  position integer,                     -- explicit (ordered), derived (scored), within-tier (tiered)
  score double precision,               -- for scored boards
  tier text,                            -- for tiered boards (key from tier_config)
  metadata jsonb,                       -- version-specific context about this entry
  UNIQUE (version_id, entry_id)
);

CREATE INDEX idx_placements_version_id ON placements(version_id);
CREATE INDEX idx_placements_entry_id ON placements(entry_id);
```

### board_references

Named `board_references` because `references` is a SQL reserved keyword.

```sql
CREATE TABLE board_references (
  id uuid PRIMARY KEY DEFAULT uuidv7(),
  board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  pinned_version_id uuid REFERENCES versions(id) ON DELETE SET NULL,
  uri text NOT NULL,                    -- "/blog/sandwich-rankings", "https://forestroyale.rileyleff.com"
  ref_type text NOT NULL CHECK (ref_type IN ('embed', 'citation', 'context')),
  label text,                           -- optional display name for this reference
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_board_references_board_id ON board_references(board_id);
```

### accumulated_scores

For accumulative boards (games), scores stream in between snapshots:

```sql
CREATE TABLE accumulated_scores (
  id uuid PRIMARY KEY DEFAULT uuidv7(),
  board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  entry_id uuid NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
  score double precision NOT NULL,
  submitted_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (board_id, entry_id)           -- one score per entry, upserted on new submission
);

CREATE INDEX idx_accumulated_scores_board_id ON accumulated_scores(board_id);
```

When `POST /boards/:slug/snapshot` is called, the service:
1. Reads all `accumulated_scores` for the board
2. Sorts by score (respecting `sort_direction`)
3. Creates a new version with derived positions
4. Does NOT clear accumulated_scores (they remain as "current standings" for the next snapshot)

This keeps score submission as a fast single-row upsert (no version creation overhead), while snapshots are explicit, deliberate operations.

---

## API Endpoints

### Boards

| Method | Path | Description |
|--------|------|-------------|
| POST | `/boards` | Create a board |
| GET | `/boards` | List boards (paginated) |
| GET | `/boards/:slug` | Board metadata + latest version summary |
| PATCH | `/boards/:slug` | Update board metadata, name, tier_config |
| DELETE | `/boards/:slug` | Delete board and all associated data |

### Entries

| Method | Path | Description |
|--------|------|-------------|
| POST | `/boards/:slug/entries` | Create an entry on a board |
| GET | `/boards/:slug/entries` | List all entries for a board |
| GET | `/boards/:slug/entries/:entry_slug` | Get entry details |
| PATCH | `/boards/:slug/entries/:entry_slug` | Update entry name/metadata |
| DELETE | `/boards/:slug/entries/:entry_slug` | Remove entry from board |
| GET | `/boards/:slug/entries/:entry_slug/history` | Entry's placement across all versions |

### Versions

| Method | Path | Description |
|--------|------|-------------|
| POST | `/boards/:slug/versions` | Create a new version (with placements) |
| GET | `/boards/:slug/versions` | List versions (paginated, newest first) |
| GET | `/boards/:slug/versions/:v` | Get a specific version with all placements |
| GET | `/boards/:slug/latest` | Shorthand for the current version with placements |
| GET | `/boards/:slug/diff` | Diff between two versions (`?from=2&to=5`) |
| GET | `/boards/:slug/since/:v` | Versions created after version v (for staleness checks) |

### Scores (accumulative boards only)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/boards/:slug/scores` | Submit a score (creates entry if needed) |
| POST | `/boards/:slug/snapshot` | Snapshot current accumulated scores as a new version |

### References

| Method | Path | Description |
|--------|------|-------------|
| POST | `/boards/:slug/references` | Register a reference |
| GET | `/boards/:slug/references` | List references for a board |
| DELETE | `/boards/:slug/references/:id` | Remove a reference |

### Sync

| Method | Path | Description |
|--------|------|-------------|
| POST | `/webhooks/github` | GitHub push webhook — triggers sync from configured repo_path |

### System

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |

---

## Key Flows

### Curated Board via Git: "Best DC Sandwiches"

1. You create files in your boards git repo:

   `boards/dc-sandwiches/board.toml`:
   ```toml
   name = "Best Sandwiches in DC"
   board_type = "ordered"

   [metadata]
   description = "A definitive and correct ranking."
   ```

   `boards/dc-sandwiches/rankings.toml`:
   ```toml
   [[entries]]
   slug = "crunchy-boi"
   name = "Compliments Only Crunchy Boi"
   position = 1

   [[entries]]
   slug = "humberto"
   name = "Dupont Market Humberto"
   position = 2

   [[entries]]
   slug = "a-litteri"
   name = "A. Litteri Italian"
   position = 3
   ```

2. You push. GitHub fires the webhook. riley_leaderboards syncs:
   - Creates the board (first time) or confirms it exists
   - Creates entries that don't exist yet
   - Creates version 1 with the placements
   - Version note comes from the git commit message

3. Blog post publishes, registers a reference pinned to version 1:
   ```
   POST /boards/dc-sandwiches/references
   { "pinned_version_number": 1, "uri": "/blog/dc-sandwiches", "ref_type": "embed" }
   ```

4. Months later, you update `rankings.toml` — put Crunchy Boi at #1, add Mangialardo's:

   ```toml
   [[entries]]
   slug = "crunchy-boi"
   name = "Compliments Only Crunchy Boi"
   position = 1

   [[entries]]
   slug = "mangialardos"
   name = "Mangialardo & Sons"
   position = 2

   [[entries]]
   slug = "humberto"
   name = "Dupont Market Humberto"
   position = 3

   [[entries]]
   slug = "a-litteri"
   name = "A. Litteri Italian"
   position = 4
   ```

5. `git commit -m "Crunchy Boi to #1, added Mangialardo's" && git push`
   Webhook fires, sync runs, version 2 is created with note from commit message.

6. Frontend rendering the blog post checks for staleness:
   ```
   GET /boards/dc-sandwiches/since/1
   → [{ version_number: 2, note: "Crunchy Boi to #1, added Mangialardo's", created_at: "..." }]
   ```
   Shows "Rankings updated since this post — view latest."

7. User clicks through. Frontend fetches `/boards/dc-sandwiches/versions/1` (pinned) and `/boards/dc-sandwiches/latest` (current). Renders slider, diff, whatever it wants.

### Accumulative Board via API: "Forest Royale High Scores"

1. Create board (once, via API or via file sync with `accumulative = true`):
   ```
   POST /boards
   { "slug": "forest-royale", "name": "Forest Royale High Scores", "board_type": "scored", "accumulative": true }
   ```

2. Game server submits scores as they happen:
   ```
   POST /boards/forest-royale/scores
   { "entry_slug": "rileyleff", "entry_name": "rileyleff", "score": 847 }
   ```
   Upserts the entry and stores the score. No version is created yet.

3. Periodic snapshot (cron, or game server calls after a round ends):
   ```
   POST /boards/forest-royale/snapshot
   { "note": "Daily standings, 2026-02-22" }
   ```
   Creates a new version from current accumulated scores, sorted by score.

4. Game frontend always shows latest:
   ```
   GET /boards/forest-royale/latest
   ```

### Tiered Board: "2026 NFL Draft Prospects"

1. `boards/nfl-draft-2026/board.toml`:
   ```toml
   name = "2026 NFL Draft Prospect Rankings"
   board_type = "tiered"

   [[tiers]]
   key = "elite"
   label = "Elite (Top 5 Pick)"

   [[tiers]]
   key = "first_round"
   label = "First Round"

   [[tiers]]
   key = "second_round"
   label = "Day 2"

   [[tiers]]
   key = "sleeper"
   label = "Sleeper"
   ```

2. `boards/nfl-draft-2026/rankings.toml`:
   ```toml
   [[entries]]
   slug = "travis-hunter"
   name = "Travis Hunter"
   tier = "elite"
   position = 1

   [[entries]]
   slug = "shedeur-sanders"
   name = "Shedeur Sanders"
   tier = "elite"
   position = 2

   [[entries]]
   slug = "cam-ward"
   name = "Cam Ward"
   tier = "first_round"
   position = 1

   [[entries]]
   slug = "tetairoa-mcmillan"
   name = "Tetairoa McMillan"
   tier = "first_round"
   position = 2
   ```

3. Push. Version 1 created. After the combine, edit the file — move Cam Ward to elite tier, push again. Version 2.

4. Frontend fetches entry history to animate movement:
   ```
   GET /boards/nfl-draft-2026/entries/cam-ward/history
   → [
       { "version_number": 1, "tier": "first_round", "position": 1, "created_at": "..." },
       { "version_number": 2, "tier": "elite", "position": 2, "created_at": "..." }
     ]
   ```
   Consumer renders the tier jump animation however it wants.

### Diff Between Versions

```
GET /boards/dc-sandwiches/diff?from=1&to=2
→ {
    "from_version": 1,
    "to_version": 2,
    "added": [{ "entry_slug": "mangialardos", "to_position": 2 }],
    "removed": [],
    "moved": [
      { "entry_slug": "humberto", "from_position": 2, "to_position": 3 },
      { "entry_slug": "a-litteri", "from_position": 3, "to_position": 4 }
    ],
    "unchanged": [
      { "entry_slug": "crunchy-boi", "position": 1 }
    ]
  }
```

For tiered boards, `moved` includes tier changes:
```json
{
  "entry_slug": "cam-ward",
  "from_tier": "first_round", "from_position": 1,
  "to_tier": "elite", "to_position": 2
}
```

---

## Auth Model

riley_leaderboards supports two mutually exclusive auth modes, configured in TOML:

### JWT mode (with riley_auth)

```toml
[auth]
jwks_url = "https://auth.rileyleff.com/.well-known/jwks.json"
required_role = "admin"
```

- Read endpoints are public (no auth required)
- Write endpoints require a valid JWT with the specified role
- The service fetches the JWKS on startup and caches it (with periodic refresh)
- No direct dependency on riley_auth — any JWT issuer that publishes a JWKS works

### API token mode (standalone)

```toml
[auth]
api_token = "env:LEADERBOARDS_API_TOKEN"
```

- Read endpoints are public
- Write endpoints require `Authorization: Bearer <token>` matching the configured token
- Simple shared secret — suitable for single-service deployments or game servers

### No auth mode

If `[auth]` is omitted entirely, all endpoints are open. Useful for development or trusted internal networks.

### Webhook authentication

The webhook endpoint uses a separate HMAC-SHA256 secret (`[sync] webhook_secret`) to verify that push events come from GitHub. This is independent of the API auth — the webhook doesn't use JWT or API tokens, it validates the `X-Hub-Signature-256` header.

---

## CLI Commands

```
riley-leaderboards serve                              # start HTTP server (runs migrations first)
riley-leaderboards migrate                            # run database migrations
riley-leaderboards validate                           # check config and DB connectivity
riley-leaderboards sync [path] [--note "..."]         # sync boards from directory (defaults to configured repo_path)
riley-leaderboards list-boards                        # list all boards
riley-leaderboards delete-board <slug>                # delete a board and all data
riley-leaderboards list-versions <slug>               # list versions for a board
riley-leaderboards export <slug>                      # export a board as JSON (all versions)
riley-leaderboards import <file>                      # import a board from JSON export
```

The `sync` command:
- Reads all board directories from the given path (or `[sync] repo_path` from config)
- Connects to the API (uses configured `api_token` or `jwks_url` for auth, or connects directly to DB if running locally)
- For each board: diffs file state against current API state, creates versions if changed
- `--note` overrides the version note for all boards updated in this sync
- Exit code 0 if all boards synced successfully, non-zero on any failure

---

## Deployment

### Dockerfile

```dockerfile
FROM rust:1.88 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates git && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/riley-leaderboards /usr/local/bin/
EXPOSE 8082
CMD ["riley-leaderboards", "serve"]
```

Note: `git` is included in the runtime image for the webhook handler to pull updates.

### docker-compose (in rileyleff deploy repo)

```yaml
leaderboards:
  build:
    context: /opt/riley-leaderboards
  environment:
    - DATABASE_URL=postgres://riley:${DB_PASSWORD}@postgres/riley
    - RILEY_LEADERBOARDS_DB_SCHEMA=leaderboards
    - WEBHOOK_SECRET=${LEADERBOARDS_WEBHOOK_SECRET}
  volumes:
    - leaderboards_boards:/data/boards    # git checkout of boards repo
  networks:
    - web
    - internal
  depends_on:
    postgres:
      condition: service_healthy
```

### Caddy

```
# Add to existing Caddyfile
rileyleff.com {
    handle /api/leaderboards/* {
        reverse_proxy leaderboards:8082
    }
}
```

---

## What's NOT in v1

- Voting / user-submitted scores with auth (users voting on rankings) — v2
- WebSocket subscriptions for live score updates — v2
- Board permissions (public/private/unlisted) — v2, open by default for now
- Board collaboration (multiple curators) — v2
- Computed/formula boards (board whose rankings derive from other boards) — v2
- Image/thumbnail generation for social sharing — v2
- RSS feeds for board updates — v2
- Entry merge/split (combine two entries or split one) — v2, for now just rename
- Admin web UI — v2, use CLI + git workflow for now

---

## Implementation Phases

### Phase 1: Foundation
- Cargo workspace setup (core, api, cli crates)
- Config loading (TOML, env var resolution, config file search)
- Database connection with configurable schema (see ~/Documents/dev/website/planning/database_config.md)
- Migration runner
- Health check endpoint
- CLI skeleton (serve, migrate, validate)

### Phase 2: Boards + Entries + Versions (curated)
- Board CRUD (create, read, update, delete)
- Entry CRUD (create, read, update, delete)
- Version creation with placements (ordered boards first)
- Version listing, fetching, latest
- Scored board support (position derived from score)
- Tiered board support (tier_config, tier placements)

### Phase 3: History + Diffing
- Entry history endpoint
- Version diff endpoint (added, removed, moved, unchanged)
- `since/:v` endpoint for staleness checks

### Phase 4: References
- Reference CRUD
- Query references by board

### Phase 5: Accumulative Boards
- accumulated_scores table
- Score submission endpoint (upsert)
- Snapshot endpoint (scores → version)

### Phase 6: File Sync
- TOML file parser for board.toml and rankings.toml
- Diff logic: file state vs current API state
- CLI `sync` command (calls API)
- GitHub webhook endpoint (verifies signature, pulls repo, triggers sync)
- Version notes from commit messages

### Phase 7: Auth
- JWT validation (JWKS fetch + cache)
- API token validation
- Auth middleware on write endpoints
- No-auth mode passthrough

### Phase 8: CLI + Polish
- list-boards, delete-board, list-versions commands
- export/import commands (JSON)
- Rate limiting
- Request logging
- CORS configuration
- Pagination on all list endpoints (cursor-based)

### Phase 9: Docker + Integration Tests
- Dockerfile (multi-stage build)
- docker-compose fragment for deploy repo
- Integration tests against real Postgres
- Caddy routing config
