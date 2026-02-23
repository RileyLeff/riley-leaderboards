# Fixes for Review Round 9 — Phase 3 R2 (2026-02-23)

**Commit**: b55c822

## Minor Fixes

1. **Shared reqwest::Client singleton** — Replaced per-delivery `Client::new()` with a `LazyLock<reqwest::Client>` static that's initialized once with `timeout(10s)`. All deliveries reuse the same client, preserving connection pools and TLS sessions. (outbound_webhooks.rs)

2. **unwrap_or_default() → expect()** — `build()` now uses `.expect()` (via LazyLock initialization), so a TLS backend failure is caught at first use rather than silently degrading. (outbound_webhooks.rs)
