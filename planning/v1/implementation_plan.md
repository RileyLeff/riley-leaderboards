# v1 Implementation Plan

Derived from `planning/v1/plan.md`. Each phase maps to a section of the plan. Steps within a phase are atomic units of work, each resulting in one or more commits.

---

## Phase 1: Foundation

**Goal:** Bootable service that connects to Postgres, runs migrations, and responds to health checks.

- **1.1** Cargo workspace setup — root `Cargo.toml`, three crate stubs (`core`, `api`, `cli`), shared dependencies
- **1.2** Config loading — TOML config struct with `env:` variable resolution, config file search (CLI flag > env var > cwd > walk up > ~/.config > /etc)
- **1.3** Database connection with configurable schema — `DatabaseConfig` with optional `schema` field, `after_connect` for `search_path`, `CREATE SCHEMA IF NOT EXISTS` (see `database_config.md`)
- **1.4** Migration runner — sqlx migrations directory, initial schema migration (all tables from plan)
- **1.5** Health check endpoint — `GET /health` returns 200
- **1.6** CLI skeleton — clap with `serve`, `migrate`, `validate` subcommands
- **1.7** Tests — config resolution, DB connection with/without schema, migration, health check

**Review:** Exhaustive review after Phase 1 completion.

---

## Phase 2: Boards + Entries + Versions

**Goal:** Full CRUD for the core data model. Curated boards work end-to-end via API.

- **2.1** Board CRUD — `POST /boards`, `GET /boards`, `GET /boards/:slug`, `PATCH /boards/:slug`, `DELETE /boards/:slug`
- **2.2** Entry CRUD — `POST /boards/:slug/entries`, `GET /boards/:slug/entries`, `GET /boards/:slug/entries/:entry_slug`, `PATCH /boards/:slug/entries/:entry_slug`, `DELETE /boards/:slug/entries/:entry_slug`
- **2.3** Version creation (ordered boards) — `POST /boards/:slug/versions` with placements, auto-increment version number
- **2.4** Version reading — `GET /boards/:slug/versions`, `GET /boards/:slug/versions/:v`, `GET /boards/:slug/latest`
- **2.5** Scored board support — position derived from score, `sort_direction` respected
- **2.6** Tiered board support — `tier_config` validation, tier placements, optional within-tier ordering
- **2.7** Tests — CRUD operations for all board types, version creation, placement validation, error cases

**Review:** Exhaustive review after Phase 2 completion.

---

## Phase 3: History + Diffing

**Goal:** Entry movement tracking and version comparison.

- **3.1** Entry history — `GET /boards/:slug/entries/:entry_slug/history` returns placements across all versions
- **3.2** Version diff — `GET /boards/:slug/diff?from=N&to=M` returns added, removed, moved, unchanged entries (with tier changes for tiered boards)
- **3.3** Staleness check — `GET /boards/:slug/since/:v` returns versions after v
- **3.4** Tests — history across multiple versions, diff for all board types, edge cases (entry added/removed, tier changes)

**Review:** Standard review after Phase 3.

---

## Phase 4: References

**Goal:** External systems can register and query cross-references to board versions.

- **4.1** Reference CRUD — `POST /boards/:slug/references`, `GET /boards/:slug/references`, `DELETE /boards/:slug/references/:id`
- **4.2** Pinned version resolution — reference with `pinned_version_number` resolves to version ID, null means "follow latest"
- **4.3** Tests — reference creation, querying, deletion, staleness with references

**Review:** Standard review after Phase 4.

---

## Phase 5: Accumulative Boards

**Goal:** Game-style leaderboards where scores stream in and versions are periodic snapshots.

- **5.1** Accumulated scores table and score submission — `POST /boards/:slug/scores` upserts entry + score
- **5.2** Snapshot — `POST /boards/:slug/snapshot` creates a new version from accumulated scores
- **5.3** Validation — accumulative endpoints reject non-accumulative boards and vice versa
- **5.4** Tests — score submission, upsert behavior, snapshot creation, sort direction, rejection of curated-only operations on accumulative boards

**Review:** Exhaustive review after Phase 5 (all data model features complete).

---

## Phase 6: File Sync

**Goal:** Curated boards can be managed via TOML files in a git repo.

- **6.1** TOML file parser — parse `board.toml` and `rankings.toml` into board/entry/placement structs
- **6.2** Diff logic — compare parsed file state against current API state, determine if a new version is needed
- **6.3** CLI `sync` command — reads directory, calls API, creates/updates boards and versions, `--note` flag
- **6.4** GitHub webhook endpoint — `POST /webhooks/github`, HMAC-SHA256 verification, pull repo, trigger sync, commit message as version note
- **6.5** Tests — file parsing (all board types), diff detection (no-op vs change), sync end-to-end, webhook signature verification

**Review:** Standard review after Phase 6.

---

## Phase 7: Auth

**Goal:** Write endpoints are gated by JWT or API token.

- **7.1** JWT validation — JWKS fetch + cache with periodic refresh, verify signature and claims
- **7.2** API token validation — constant-time comparison against configured token
- **7.3** Auth middleware — applied to write endpoints, checks JWT or API token depending on config, passthrough if no auth configured
- **7.4** Webhook auth — separate HMAC-SHA256 verification (independent of API auth)
- **7.5** Tests — JWT validation (valid, expired, wrong role), API token validation, no-auth passthrough, middleware integration

**Review:** Standard review after Phase 7.

---

## Phase 8: CLI + Polish

**Goal:** Admin CLI commands, pagination, rate limiting, production hardening.

- **8.1** Admin CLI commands — `list-boards`, `delete-board`, `list-versions`
- **8.2** Export/import — `export <slug>` dumps board as JSON (all versions), `import <file>` restores
- **8.3** Pagination — cursor-based pagination on all list endpoints
- **8.4** Rate limiting — configurable per-IP rate limits
- **8.5** CORS — configurable origins from config
- **8.6** Request logging — structured tracing
- **8.7** Tests — pagination edge cases, rate limiting, export/import round-trip

**Review:** Standard review after Phase 8.

---

## Phase 9: Docker + Integration Tests

**Goal:** Deployable container, full integration test suite.

- **9.1** Dockerfile — multi-stage build (rust builder → debian slim runtime with git)
- **9.2** docker-compose fragment — for deploy repo integration
- **9.3** Integration tests — full test suite against real Postgres in Docker, covering all phases
- **9.4** Caddy config snippet — reverse proxy routing

**Review:** Exhaustive review (final milestone).
