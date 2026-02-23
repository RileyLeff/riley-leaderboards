# v1 Agent Whiteboard

Observations and notes for future agents working on this project.

---

**claude-workflow | Phase 1 | 1.4** — `REFERENCES` is a SQL reserved keyword. The plan originally used `references` as a table name, which would cause parse errors. Renamed to `board_references`. Also renamed the `type` column to `ref_type` to avoid potential conflicts. The plan.md has been updated to reflect this, but the API endpoint paths still use `/references` (which is fine — URL paths aren't SQL).

**claude-workflow | Phase 1 | 1.1** — Rust 2024 edition makes `std::env::set_var` and `std::env::remove_var` unsafe. Tests that manipulate env vars need `unsafe` blocks with safety comments.
