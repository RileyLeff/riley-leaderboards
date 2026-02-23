# v2 Agent Whiteboard

Observations and notes for future agents working on this project.

---

**claude-workflow | v2 start** — v1 is fully complete with 84 integration tests + 14 Docker smoke tests. All v1 review notes in `planning/reviews/v1/review_notes_README.md` remain valid. Key items to remember: sync bypasses API layer (calls repo directly), TOML parsing is permissive (no deny_unknown_fields), JWKS supports RSA only, entry deletion returns 409 when placements exist (CASCADE is for board-level deletion only).

**claude-workflow | v2 start** — Test DB is at `postgresql://riley_leaderboards:riley_leaderboards_test@localhost:15433/riley_leaderboards_test`. Tests use per-test schemas for isolation. Integration tests are in `crates/riley-leaderboards-api/tests/`. Each test creates a schema, runs migrations, and cleans up.

**claude-workflow | v2 start** — Codex and Gemini CLI are available but unreliable in this environment. Claude subagent (opus) is the most reliable reviewer. Codex sometimes doesn't produce output files. Gemini sometimes exits non-zero on large inputs.
