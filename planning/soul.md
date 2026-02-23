# Soul

riley_leaderboards treats rankings as living documents.

Most systems store a leaderboard as a flat table — here's the current state, that's all you get. But rankings are interesting *because* they change. The sandwich shop that was #3 in February climbed to #1 by summer. The draft prospect nobody had heard of in October is a first-round lock by April. The fun isn't just where things are — it's where they were, and how they got here.

riley_leaderboards makes that history a first-class concept. Every state is a version. Versions are immutable. You can pin to a moment in time, diff between two moments, trace a single entry's journey across all of them. The service doesn't just answer "what's the ranking?" — it answers "what changed, when, and what was the world like when this was published?"

## Principles

**Rankings change. Capture that.** Every edit creates a new version. Nothing is overwritten. A board is not its current state — it's its entire history. Consumers decide what to show: the latest, the pinned, the diff, the trajectory.

**The version is the atomic unit.** A version is a complete snapshot, not a delta. You never reconstruct state by replaying changes — any version is self-contained and can be fetched directly. This keeps reads simple and writes append-only.

**One set of primitives, many shapes.** Sandwich rankings, game high scores, tier lists, draft boards — these feel like different things but they're structurally similar: entities with positions that change over time. The board type system captures the meaningful differences without fragmenting the data model.

**Data in, data out.** The service stores and serves versioned rankings. It has no opinions about how they're displayed — no rendering, no styling, no interaction logic. A frontend might show a slider, animate tier jumps, or render a static list. That's the consumer's domain, not the service's.

**The library is the product.** riley_leaderboards is not "the leaderboard for Riley's website." It's a leaderboard service that Riley's website happens to use. The API, the config format, the data model — these should make sense to someone who has never heard of rileyleff.com.

**Configuration over code.** Board types, tier labels, sort direction, auth mode, database topology — these are deployment decisions. Two people running riley_leaderboards should have completely different boards and policies without touching the source code.
