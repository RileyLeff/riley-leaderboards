# riley-leaderboards

[![CI](https://github.com/RileyLeff/riley-leaderboards/actions/workflows/ci.yml/badge.svg)](https://github.com/RileyLeff/riley-leaderboards/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/riley-leaderboards-core.svg)](https://crates.io/crates/riley-leaderboards-core)
[![docs.rs](https://docs.rs/riley-leaderboards-core/badge.svg)](https://docs.rs/riley-leaderboards-core)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/riley-leaderboards-core.svg)](LICENSE-MIT)

A general-purpose versioned ranking service.

Most systems store a leaderboard as a flat table -- here's the current state, that's all you get. riley-leaderboards treats rankings as living documents. Every edit creates a new immutable version. You can diff between two moments, pin to a specific version, or trace a single entry's journey across all of them.

## Features

- **Board types** -- ordered (explicit positions), scored (auto-ranked by score), tiered (tier lists), and accumulative (streaming score ingestion with snapshots)
- **Immutable versioning** -- every state is a version, nothing is overwritten, diffs between any two versions
- **Realtime** -- Redis-backed live scoring with Server-Sent Events for push updates
- **Collections** -- group boards together for cross-board views
- **Auth** -- JWT (JWKS) or API token, with optional read-only tokens and per-endpoint auth granularity
- **Git sync** -- define boards as TOML files in a git repo, sync on push via GitHub webhooks
- **Outbound webhooks** -- fire-and-forget notifications on board/version events with HMAC signing
- **Prometheus metrics** -- request duration, counters, active SSE connections, domain metrics
- **OpenAPI / Swagger UI** -- auto-generated spec at `/docs`
- **Rate limiting** -- per-IP with configurable burst, reverse-proxy aware
- **Docker-ready** -- multi-stage Dockerfile included

## Quickstart

### From source

```sh
cargo install --path crates/riley-leaderboards-cli
cp riley_leaderboards.example.toml riley_leaderboards.toml
# Edit riley_leaderboards.toml -- set database.url at minimum
riley-leaderboards migrate
riley-leaderboards serve
```

### With Docker

```sh
docker build -t riley-leaderboards .
docker run -e DATABASE_URL=postgresql://... -p 8082:8082 riley-leaderboards
```

Requires PostgreSQL 18+ (uses native `uuidv7()`). Redis is optional, needed only for realtime boards.

## Configuration

All configuration lives in a single TOML file. The service searches for `riley_leaderboards.toml` in the current directory and `/etc/riley_leaderboards/config.toml`. Override with `--config path/to/file.toml`.

See [`riley_leaderboards.example.toml`](riley_leaderboards.example.toml) for all options. Key sections:

| Section | Purpose |
|---------|---------|
| `[server]` | Host, port, CORS, rate limiting, SSE, metrics, docs, shutdown timeout |
| `[database]` | Postgres connection URL, pool size, optional schema isolation |
| `[redis]` | Redis URL and key prefix (for realtime boards) |
| `[auth]` | JWT (JWKS URL + role) or API token, read-only tokens, read auth toggle |
| `[sync]` | Git repo path, webhook secret, branch filter |
| `[limits]` | Max entries per version, max versions per board, metadata size |
| `[[webhooks]]` | Outbound webhook URLs, event filters, board glob patterns, HMAC secrets |

Secrets can be inlined or loaded from environment variables with the `env:VAR_NAME` syntax.

## CLI

| Command | Description |
|---------|-------------|
| `serve` | Start the HTTP server |
| `migrate` | Run database migrations |
| `validate` | Check config and database connectivity |
| `sync [path]` | Sync boards from a directory of TOML files |
| `list-boards` | List all boards |
| `delete-board <slug>` | Delete a board and all its data |
| `list-versions <slug>` | List versions for a board |
| `export <slug>` | Export a board as JSON (all versions with placements) |
| `import <file>` | Import a board from a JSON export file |
| `list-collections` | List all collections |
| `delete-collection <slug>` | Delete a collection (does not delete its boards) |

## API

When `docs_enabled = true` (the default), Swagger UI is available at `/docs` and the OpenAPI spec at `/api-doc/openapi.json`.

Key endpoints:

```
GET    /health
POST   /boards                          Create a board
GET    /boards/{slug}                   Get a board
GET    /boards/{slug}/versions          List versions
POST   /boards/{slug}/versions          Create a version (snapshot of placements)
GET    /boards/{slug}/latest            Get the latest version with placements
GET    /boards/{slug}/diff?from=1&to=3  Diff between two versions
GET    /boards/{slug}/stream            SSE stream (realtime boards)
POST   /boards/{slug}/scores            Submit a score (accumulative boards)
POST   /boards/{slug}/snapshot          Snapshot accumulated scores into a version
```

Write operations require a Bearer token. Read operations are public by default (configurable with `require_read_auth`).

## Board Types

**Ordered** -- entries have explicit integer positions. You supply the ranking directly.

**Scored** -- entries have numeric scores. Positions are derived automatically based on `sort_direction` (desc or asc).

**Tiered** -- entries are placed into named tiers (S/A/B/C or whatever you configure). Within a tier, entries are unranked.

**Accumulative** -- a scored board that accepts streaming score submissions. Scores accumulate in Redis until you trigger a snapshot, which freezes them into an immutable version. Supports realtime SSE push to connected clients.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
