# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-02-24

### Added

- **WebSocket streaming**: alternative to SSE for live board updates (`GET /boards/:slug/ws`), configurable via `ws_enabled` and `ws_timeout_secs`
- **Bidirectional WebSocket**: clients can submit scores over the same WS connection they receive events on (realtime boards only)

### Changed

- Renamed internal `SseEvent` to `BoardEvent` to reflect transport-agnostic design
- Renamed Prometheus metric `sse_active_connections` to `streaming_active_connections`

## [0.1.1] - 2026-02-23

### Fixed

- Docker build: updated migrations path after moving into core crate
- CI: webhook test now uses explicit `--initial-branch=main` for git portability
- CI: webhook test uses TaskTracker instead of sleep for reliable background task waiting
- crates.io: all sub-crates now include the workspace README
- README: added CI, crates.io, docs.rs, and license badges

## [0.1.0] - 2025-02-23

### Added

- **Board types**: ordered (explicit positions), scored (auto-ranked by score), tiered (named tiers with configurable labels)
- **Accumulative boards**: streaming score ingestion with snapshot-to-version workflow
- **Immutable versioning**: every board state is a numbered version; diffs between any two versions; entry history tracking across versions
- **Realtime boards**: Redis-backed live scoring with Server-Sent Events (SSE) push to connected clients
- **Collections**: group boards together with ordered membership
- **Authentication**: JWT via JWKS endpoint (with optional issuer/audience validation), API token mode, read-only tokens, configurable read auth
- **Git sync**: define boards as TOML files in a git repo; sync on push via GitHub webhook integration
- **Outbound webhooks**: fire-and-forget notifications on board/version events with optional HMAC-SHA256 signing and board glob filters
- **Prometheus metrics**: request duration histograms, request counters, domain metrics (versions created, scores submitted, webhook deliveries, active SSE connections)
- **OpenAPI / Swagger UI**: auto-generated spec at `/api-doc/openapi.json`, interactive docs at `/docs`
- **Rate limiting**: per-IP with configurable burst size, reverse-proxy aware (X-Forwarded-For)
- **CLI**: `serve`, `migrate`, `validate`, `sync`, `list-boards`, `delete-board`, `list-versions`, `export`, `import`, `list-collections`, `delete-collection`
- **Configurable schema isolation**: run multiple instances against a single Postgres database
- **Graceful shutdown**: in-flight webhook deliveries and background tasks drain with configurable timeout
- **Docker support**: multi-stage Dockerfile with dependency caching
- **Board references**: link boards to external URIs with optional version pinning
- **Export/import**: full board serialization (all versions with placements) as JSON
- **Pagination**: cursor-based pagination for boards, entries, collections, and versions
