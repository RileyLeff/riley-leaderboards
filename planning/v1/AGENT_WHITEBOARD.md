# v1 Agent Whiteboard

Observations and notes for future agents working on this project.

---

**claude-workflow | Phase 1 | 1.4** — `REFERENCES` is a SQL reserved keyword. The plan originally used `references` as a table name, which would cause parse errors. Renamed to `board_references`. Also renamed the `type` column to `ref_type` to avoid potential conflicts. The plan.md has been updated to reflect this, but the API endpoint paths still use `/references` (which is fine — URL paths aren't SQL).

**claude-workflow | Phase 1 | 1.1** — Rust 2024 edition makes `std::env::set_var` and `std::env::remove_var` unsafe. Tests that manipulate env vars need `unsafe` blocks with safety comments.

**claude-workflow | Phase 1 | review** — Exhaustive review converged in 3 rounds (1 major in R1 → 0 major in R2+R3). Key fix: schema creation must happen before pool construction (bootstrap connection pattern). Gemini CLI is not available in this environment but Gemini did produce output via background shell. Codex runs but doesn't reliably produce the output file. Claude subagent is the most reliable reviewer.

**claude-workflow | Phase 1 | review** — ref_type CHECK constraint uses semantic relationship types (embed/citation/context) not entity types (blog_post/game/page). The plan has been updated to match. This was a deliberate improvement over the original plan.

**claude-workflow | Phase 1 | review** — The plan.md DB schema section still shows indexes that have been removed from the actual migration (idx_entries_board_id, idx_versions_board_id_number, idx_placements_version_id, idx_accumulated_scores_board_id). These are redundant with UNIQUE constraint indexes. The plan should be updated to match the implementation when convenient, but this is cosmetic.
