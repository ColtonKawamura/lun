# lun schema (Phase 2)

Canonical storage is a single SQLite file at `.lun/lun.db` in the current
working directory (one DB per tracked project/repo). Markdown is a view,
never a source of truth.

## Conventions

- **Timestamps**: UTC, `YYYY-MM-DDTHH:MM:SSZ`.
- **IDs**: `INTEGER PRIMARY KEY` (rowid alias). Human-facing keys are the
  separate `project_key` / `task_key` columns; raw ids are internal.
- **Keys**: `P-001`, `T-008` style. `P-000` is reserved for the seeded
  "Unassigned" project; real projects start at `P-001` and tasks at `T-001`,
  both auto-incremented from the next free number.
- **JSON**: `labels` and `details` are serialized JSON stored as TEXT
  (arrays of strings / objects), kept small so agents can query subsets
  without parsing big blobs.
- **Foreign keys**: enforced (`PRAGMA foreign_keys = ON`). WAL journal mode.

## Migrations

Schema changes go through a `migrations` table; `lun init` applies any
pending migration and is idempotent. `lun` refuses to run against a DB
newer than the binary expects (version mismatch error).

```sql
CREATE TABLE migrations (
    version    INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
```

Version 1 (initial schema) creates the tables below and seeds P-000.

## projects

| column        | type    | notes |
| ------------- | ------- | ----- |
| id            | INTEGER | PK |
| project_key   | TEXT    | UNIQUE, e.g. `P-001`; `P-000` = Unassigned (seeded) |
| name          | TEXT    | e.g. `paper-stack` |
| status        | TEXT    | `planning` \| `active` \| `in-progress` \| `done` (CHECK) |
| created_at    | TEXT    | UTC timestamp |
| updated_at    | TEXT    | UTC timestamp |

Seed row: `P-000 / "Unassigned" / active`.

## tasks

| column     | type    | notes |
| ---------- | ------- | ----- |
| id         | INTEGER | PK |
| task_key   | TEXT    | UNIQUE, e.g. `T-008` |
| project_id | INTEGER | FK → `projects.id`, nullable; tasks without a project are written with an explicit reference to P-000 |
| title      | TEXT    | |
| status     | TEXT    | `todo` \| `in-progress` \| `review` \| `done` (CHECK) |
| priority   | TEXT    | `low` \| `med` \| `high` (CHECK) |
| assignee   | TEXT    | nullable |
| branch     | TEXT    | nullable (git branch name) |
| labels     | TEXT    | JSON array of strings, default `[]` |
| created_at | TEXT    | UTC timestamp |
| updated_at | TEXT    | UTC timestamp |

## logs

Lun's "logical commits": **every state change writes a row here** with a
commit-style `message`.

| column      | type    | notes |
| ----------- | ------- | ----- |
| id          | INTEGER | PK |
| entity_type | TEXT    | `project` \| `task` (CHECK) |
| entity_id   | INTEGER | row id of the entity (no cross-table FK by design — the entity is the thing being described) |
| timestamp   | TEXT    | UTC timestamp |
| user        | TEXT    | actor, default `me` (seed row uses `system`) |
| action      | TEXT    | e.g. `CREATE`, `UPDATE`, `ATTACH`, `LINK`, `COMMENT` |
| message     | TEXT    | commit-style message, e.g. `add task "Tune damping" to paper-stack` |
| details     | TEXT    | JSON object with field changes/notes, default `{}` |

Actions written by the Phase 2 data-access layer: `CREATE` (project/task),
`ATTACH` (attachment added), `LINK` (link added). The P-000 seed is also
logged (`user = system`).

## attachments

| column      | type    | notes |
| ----------- | ------- | ----- |
| id          | INTEGER | PK |
| task_id     | INTEGER | FK → `tasks.id` |
| filename    | TEXT    | original file name |
| stored_path | TEXT    | path where the copy lives (`.lun/attachments/...` from Phase 4) |
| created_at  | TEXT    | UTC timestamp |

## links

| column     | type    | notes |
| ---------- | ------- | ----- |
| id         | INTEGER | PK |
| task_id    | INTEGER | FK → `tasks.id`, nullable |
| project_id | INTEGER | FK → `projects.id`, nullable |
| label      | TEXT    | e.g. `obsidian` |
| uri        | TEXT    | e.g. `obsidian://open?vault=personal&file=...` |
| created_at | TEXT    | UTC timestamp |

CHECK constraint: exactly one of `task_id` / `project_id` is set
(`(task_id IS NULL) <> (project_id IS NULL)`).

## Data-access guarantees (Phase 2)

- `Lun::init(root)` — create `.lun/`, open `lun.db`, run migrations,
  enforce version. Idempotent.
- `create_project` / `list_projects` — keys auto-increment; each create
  logs `CREATE`.
- `create_task` / `list_tasks` — keys auto-increment; missing project
  falls back to P-000; each create logs `CREATE` with the default
  commit message `add task "<title>" to <project>`.
- `add_attachment` — logs `ATTACH`; unknown `task_id` is a clean error.
- `add_link` — logs `LINK`; target is a task or a project.
- `log` — direct entry point for non-mutating records (e.g. comments in
  a later phase).
