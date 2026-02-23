# Fixes for Review Round 34 (R3)

**Commit:** a61a75f

## Minor Fixes

1. **Empty tiers array rejected** — Added `tiers.is_empty()` check in `validate_tier_config()`. Tiered boards with no tiers defined are now properly rejected at creation/update time.

## Notes Disposition

All 9 notes from R3 are either:
- Already documented in `review_notes_README.md` (CORS, behind_proxy, duplicate tier keys)
- Confirmed as intentional design (JWKS fail-closed, webhook HMAC, health public)
- Acceptable v1 tradeoffs (TOCTOU in sync, diff double fetch)

No further action needed for v1.
