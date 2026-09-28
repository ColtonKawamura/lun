# PHASE 2 — lun data model & DB layer

Goal: implement the SQLite schema + data-access layer so later phases build on it. Canonical storage is the DB; markdown is never stored as truth.

Context: read docs/architecture.md, docs/plan.md (Phase 2 section has the full spec). Rust; add rusqlite (bundle feature) — no other deps unless needed.

## Tasks

1. **Schema** — tables:
   - `projects` (id, project_key P-001, name, status, created_at, updated_at)
   - `tasks` (id, task_key T-008, project_id FK nullable, title, status todo|doing|follow-up|done, priority low|med|high, assignee, branch, labels JSON, timestamps)
   - `logs` (id, entity_type project|task, entity_id, timestamp, user, action, message, details JSON)
   - `attachments` (id, task_id, filename, stored_path, created_at)
   - `links` (id, task_id/project_id, label, uri, created_at)

   Seed P-000 "Unassigned".

2. **`lun init`**: create `.lun/lun.db` in CWD; migrations table + version check; idempotent on re-run.

3. **Data-access layer**: create/list projects, create/list tasks (auto-increment T-00N), log entries, add attachment/link records. EVERY state change writes a logs entry with commit-style message.

4. **Document schema** in `docs/schema.md`.

5. **Unit tests**: init idempotency, key generation, log-on-write, P-000 fallback. Run `cargo test` — must pass.

## Rules

- Branch-first: `git checkout -b phase2-db-layer`.
- Never touch `docs/plan.md`.
- Commit this brief (`docs/phase2-brief.md`) as the first commit on the branch so the spec ships with the PR.
- Detailed commit message (what + why).
- End with a PR to main whose description is a third-person summary (what/why/verification/out-of-scope).
- Keep the CLI stub (`src/main.rs` banner) working.
