# Round 2 Fixes — 2026-02-22

## Minor

### 1. ref_type CHECK constraint values diverge from plan
**Fixed in** `fdaede2`

Updated plan.md to use the implementation's ref_type taxonomy (embed/citation/context)
instead of the original entity-type values (blog_post/game/page). The implementation's
taxonomy is more compositional.

### 2. Validate command error stringification
**Fixed in** `fdaede2`

Changed `.map_err(|e| anyhow::anyhow!("...{e}"))` to `.context("...")` throughout
main.rs. Also simplified load_config error handling to use `.context()`.
