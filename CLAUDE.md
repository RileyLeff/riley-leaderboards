## Slack

When using slack_notify or slack_ask, use channel `C0AGG0UPBMG`.

## Project Structure

Cargo workspace with three crates:
- `riley-leaderboards-core` — config, db (sqlx/postgres), models, versioning, sync logic
- `riley-leaderboards-api` — Axum HTTP server, route handlers, middleware, webhook handler
- `riley-leaderboards-cli` — CLI binary (clap), serves API and manages setup/sync

## Conventions

- Rust edition 2024, MSRV 1.88
- PostgreSQL 18 with native `uuidv7()` for primary keys, `timestamptz` for all timestamps
- Configurable database schema isolation (see `~/Documents/dev/website/planning/database_config.md`)
- All config is TOML, loaded with the same resolution pattern as riley_cms and riley_auth
- This is a general-purpose library, not "Riley's leaderboard" — APIs and config should make sense to anyone

## Architecture

See `planning/soul.md` for project philosophy.
See `planning/v1/plan.md` for the current implementation plan.

## Skills

When working on database schema, migrations, or indexes, agents MUST read and apply the `database-architecture` skill. This enforces PostgreSQL 18 best practices including UUIDv7, AIO-friendly access patterns, and modern indexing strategies.
