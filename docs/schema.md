# lun schema

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

Version 2 (Phase 7) adds `tasks.notes` (see `tasks` below):

```sql
ALTER TABLE tasks ADD COLUMN notes TEXT NOT NULL DEFAULT '';
```

Version 3 (Phase 9) adds the `prs` table (see `prs` below):

```sql
CREATE TABLE prs (
    id            INTEGER PRIMARY KEY,
    pr_key        TEXT NOT NULL UNIQUE,
    task_id       INTEGER NOT NULL REFERENCES tasks(id),
    source_branch TEXT NOT NULL,
    target_branch TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'open'
                        CHECK (status IN ('open', 'merged')),
    created_at    TEXT NOT NULL,
    merged_at     TEXT
);
```

Version 4 rebuilds `attachments` so they can belong to either a task or a
project:

```sql
ALTER TABLE attachments RENAME TO attachments_v1;
CREATE TABLE attachments (
    id          INTEGER PRIMARY KEY,
    task_id     INTEGER REFERENCES tasks(id),
    project_id  INTEGER REFERENCES projects(id),
    filename    TEXT NOT NULL,
    stored_path TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    CHECK ((task_id IS NULL) <> (project_id IS NULL))
);
INSERT INTO attachments (...)
SELECT ... FROM attachments_v1;
DROP TABLE attachments_v1;
```

Version 5 adds soft-archive state to tasks:

```sql
ALTER TABLE tasks ADD COLUMN archived_at TEXT;
```

Version 6 normalizes statuses:
- project statuses become `active` / `inactive` only;
- task statuses become `todo` / `doing` / `follow-up` / `blocked` / `done`;
- existing task values are migrated (`in-progress`→`doing`, `review`→`follow-up`);
- existing project values are migrated (`done`→`inactive`, everything else→`active`).

## projects

| column        | type    | notes |
| ------------- | ------- | ----- |
| id            | INTEGER | PK |
| project_key   | TEXT    | UNIQUE, e.g. `P-001`; `P-000` = Unassigned (seeded) |
| name          | TEXT    | e.g. `paper-stack` |
| status        | TEXT    | `active` \| `inactive` (CHECK) |
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
| status     | TEXT    | `todo` \| `doing` \| `follow-up` \| `blocked` \| `done` (CHECK) |
| priority   | TEXT    | `low` \| `med` \| `high` (CHECK) |
| assignee   | TEXT    | nullable |
| branch     | TEXT    | nullable (git branch name) |
| labels     | TEXT    | JSON array of strings, default `[]` |
| notes      | TEXT    | free-form markdown notes, default `''` (Phase 7: editable in the TUI; saved via `Lun::set_notes`, which logs `UPDATE`) |
| created_at | TEXT    | UTC timestamp |
| updated_at | TEXT    | UTC timestamp |
| archived_at | TEXT   | nullable UTC timestamp; archived tasks are hidden from normal task lists |

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
| task_id     | INTEGER | FK → `tasks.id`, nullable |
| project_id  | INTEGER | FK → `projects.id`, nullable |
| filename    | TEXT    | original file name |
| stored_path | TEXT    | path where the copy lives (`.lun/attachments/...` from Phase 4) |
| created_at  | TEXT    | UTC timestamp |

CHECK constraint: exactly one of `task_id` / `project_id` is set
(`(task_id IS NULL) <> (project_id IS NULL)`).

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

## prs

| column        | type    | notes |
| ------------- | ------- | ----- |
| id            | INTEGER | PK |
| pr_key        | TEXT    | unique, `PR-00N` style (MAX(id)-based, auto-increment) |
| task_id       | INTEGER | FK → `tasks.id` (a PR is always about one task) |
| source_branch | TEXT    | branch being merged in |
| target_branch | TEXT    | merge target (defaults to `main`) |
| status        | TEXT    | `open` \| `merged` (CHECK-constrained) |
| created_at    | TEXT    | UTC timestamp |
| merged_at     | TEXT    | UTC timestamp, set when status → `merged` |

Phase 9 (GitHub-style PRs). One open PR per task. The PR lifecycle is
recorded in the owning task's `logs` (the PR's `UPDATE`/`MERGE` entries
carry the `pr` key in `details`) because `logs.entity_type` is
CHECK-constrained to `project`/`task`.

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

## Phase 7 additions

- `tasks.notes` — free-form markdown per task, editable in the TUI.
- `Lun::set_notes(task_id, notes, message, user)` — replaces the notes,
  bumps `updated_at`, and logs `UPDATE` with the default commit message
  `update notes for <task>` (or the supplied message). `details` records
  `notes: <old> -> <new>` as a line-count description (`empty`,
  `1 line`, `2 lines`, …) — the notes text itself lives in the column,
  not in `details`.

## Phase 9 additions

- `Lun::create_pr(PrSpec)` — opens a PR for a task. `source_branch`
  defaults to the task's `branch` column (a task with no branch and no
  explicit `--from` is a `usage` error); `target_branch` defaults to
  `main`. `source == target` and a second open PR on the same task are
  `usage` errors. Key is `PR-{:03}` from MAX(id) (empty table → `PR-001`;
  numbering never reuses, even after merges). Logs `UPDATE` on the task
  with `details` = `{"pr", "source", "target"}`.
- `Lun::merge_pr(pr_id, message, user)` — flips an open PR to `merged`,
  stamps `merged_at`, moves the task to `done` (a merged PR means the
  task is done), and logs `MERGE` on the task with `details` =
  `{"changes", "pr", "source", "target"}`. Merging a non-open PR is a
  `usage` error (`already merged`); an unknown id is `not-found`.
- `list_prs` / `list_open_prs` / `pr_by_key` / `prs_for_task` /
  `open_pr_for_task` — read models. `pr_by_key` maps a missing key to
  `not-found`.
- CLI: `lun pr new|show|ls|merge` (see `main.rs` help). `pr merge` runs
  the logical merge first and then, only when the CWD is a git repo on
  the target branch and the source branch exists locally, `git merge
  --no-edit <source>`; a missing branch / wrong branch / merge failure
  is *reported* in the output but never undoes the logical merge (the DB
  log is the canonical record — the plan keeps git integration optional).
