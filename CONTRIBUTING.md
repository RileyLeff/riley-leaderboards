# Contributing

## What this project is

riley-leaderboards treats rankings as living documents. Every edit creates a new immutable version. You can diff between two moments, pin to a specific version, or trace a single entry's journey across all of them.

## Principles

**Rankings change. Capture that.** Every edit creates a new version. Nothing is overwritten. A board is not its current state -- it's its entire history.

**The version is the atomic unit.** A version is a complete snapshot, not a delta. You never reconstruct state by replaying changes -- any version is self-contained.

**One set of primitives, many shapes.** Sandwich rankings, game high scores, tier lists, draft boards -- these are structurally similar. The board type system captures the meaningful differences without fragmenting the data model.

**Data in, data out.** The service stores and serves versioned rankings. No rendering, no styling, no interaction logic. That's the consumer's domain.

**The library is the product.** This is not "a leaderboard for one website." It's a leaderboard service. The API, config format, and data model should make sense to anyone.

**Configuration over code.** Board types, tier labels, sort direction, auth mode -- these are deployment decisions, not code changes.

## What's in scope

- New board types that fit the "entities with positions that change over time" model
- Query capabilities (filtering, search, cross-board views)
- Auth enhancements (additional providers, finer-grained permissions)
- Performance work (query optimization, caching)
- Deployment tooling (Helm charts, additional container registries)
- Client libraries

## What's out of scope

- Rendering or display logic (frontends, embeds, widgets)
- User management (this service authenticates via external providers, it doesn't manage users)
- Game logic or score calculation (the service records scores, it doesn't compute them)
- Multi-database support (PostgreSQL only, by design)

## Development

```sh
# Start Postgres + Redis
docker compose up -d

# Run tests
cargo test --workspace

# Check formatting + lints
cargo fmt --check --all
cargo clippy --workspace -- -D warnings
```

Tests expect `TEST_DATABASE_URL` and `TEST_REDIS_URL`. The docker-compose defaults are:
- `postgresql://riley_leaderboards:riley_leaderboards_test@localhost:15433/riley_leaderboards_test`
- `redis://localhost:16380`
