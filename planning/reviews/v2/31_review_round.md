# Review Round 31 (R2) — 2026-02-23

**Models**: Claude Opus 4.6 only (Codex usage-limited, Gemini CLI not installed)
**Context**: ~233k tokens

## Fix Verification

All 5 R1 fixes verified correct:
- Fix #2: `"unmatched"` fallback in metrics.rs prevents cardinality explosion
- Fix #3: JWKS refresh task now tracked by TaskTracker for graceful shutdown
- Fix #4: `versions::since` OpenAPI annotation corrected to `Vec<Version>`
- Fix #8: `boards::list` OpenAPI annotation corrected to `PaginatedResponse<Board>`
- Fix #10: `collections::update` uses transaction with FOR UPDATE lock

No regressions found from any fix.

## Findings

### Major
None.

### Minor

#### 1. Inconsistent no-op guard placement between boards and collections update
- Boards: no-op check in route handler before calling repo
- Collections: no-op check inside repo function after FOR UPDATE lock
- **Action:** Fixed — moved collections no-op guard to route handler, matching boards pattern

#### 2. Health response missing component status
- Health endpoint returns `{"status": "ok"}` without distinguishing which components were checked
- Integration test checks for `.redis` field that doesn't exist (lenient fallback masks this)
- **Action:** Fixed — health response now includes `"postgres": "ok"` and `"redis": "ok"` when Redis is configured

### Notes

#### 3. ConnectionGuard::drop underflow risk (re-noted)
- `fetch_sub(1)` wraps on underflow in release mode. Previously flagged in R1 as Minor #6 but deferred without documentation.
- **Action:** Document in review_notes_README.md as accepted.

#### 4. JWKS refresh task relies on TaskTracker drop, no CancellationToken
- Task loop has no `select!` on cancellation. Relies on runtime drop.
- Acceptable because the task is tracked and the 3600s interval means it's sleeping during shutdown.

#### 5. export_board N+1 query pattern
- Each version gets separate queries. Acceptable at current scale (operator-initiated, infrequent).

#### 6. sort_direction error message slightly misleading for non-scored boards
- Error says "only meaningful for scored boards" but allows the default `"desc"` value. Cosmetic.
