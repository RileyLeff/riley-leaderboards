# Design Decisions

Intentional tradeoffs and choices that might look like bugs. If you're reviewing the code or considering a change, check here first.

## Data model

**`board_type` and `accumulative` are immutable.** These are set at creation and cannot be changed via PATCH. Changing a board's type would invalidate all existing versions. The PATCH endpoint silently ignores these fields.

**`double precision` for scores.** Acceptable for game scores and rankings. If exact-precision use cases arise, `numeric` could be considered for specific board types.

**`sort_direction` on non-scored boards is a no-op.** Changing it on ordered/tiered boards succeeds silently. The field has no effect on non-scored board logic.

**Cross-board integrity is enforced at the application level.** Version creation resolves entry slugs with `WHERE board_id = $1 AND slug = $2`. The schema theoretically allows cross-board references via raw SQL, but the service is the only writer.

## Versioning

**Versions are complete snapshots, not deltas.** This keeps reads simple and writes append-only, at the cost of storage. A version with 100 entries stores all 100, even if only one changed.

**Sync does not detect metadata-only changes.** If you update only `[version_metadata]` in a TOML file without changing placements, no new version is created. Versions capture ranking state -- metadata is context about that state.

## Auth

**JWKS supports RSA keys only.** EC (ES256/ES384/ES512) and EdDSA keys are not supported in v1.

**Omitting `required_role` allows any valid JWT to write.** The role check is an optional additional constraint, not a requirement.

**Auth token comparison uses HMAC for constant-time equality.** The HMAC key is not a secret -- it exists solely to enable `verify_slice()` for timing-safe comparison.

## Webhooks

**Outbound webhooks are fire-and-forget.** Delivery failures are logged but not retried. Consumers should be idempotent.

**CLI commands may lose in-flight webhook deliveries on exit.** Spawned tasks are cancelled when the runtime shuts down. Acceptable since CLI usage is infrequent and webhooks are best-effort.

**Sync-created boards don't fire `board.created` webhooks.** Only `version.created` fires. This avoids threading creation metadata through the sync module.

## Realtime / SSE

**No read endpoint for accumulated scores.** There is no `GET /boards/:slug/scores` preview. Snapshot materializes state -- that's the only way to "read" accumulated scores.

**Snapshot preserves accumulated scores by default.** This is the "all-time high score" pattern. Boards with `clear_on_snapshot = true` reset scores after each snapshot.

**`score.updated` SSE events omit position.** Computing position would require reading the full sorted set from Redis on every score submission. Clients that need position should poll the latest endpoint.

**No SSE connection timeout.** Connections stay open until the client disconnects. Production deployments should use a reverse proxy to enforce upstream timeouts.

## Operations

**`/metrics` and `/docs` are unauthenticated.** Production deployments should restrict access via reverse proxy. Both can be disabled via config (`metrics_enabled`, `docs_enabled`).

**Auto-migrate on `serve` is intentional.** For convenience. Use `migrate` as a standalone command for production workflows that need explicit migration control.

**Integration tests may leak schemas on panic.** If an assertion panics before cleanup, test schemas remain in the database. Harmless in development.

## Sync

**TOML parsing is permissive about unknown fields.** Typos in field names are silently ignored. This preserves forward compatibility -- strict mode would break users on upgrade.

**Sync is not atomic across boards.** If syncing the second of three boards fails, the first board's changes are already committed. Failures are logged and processing continues.
