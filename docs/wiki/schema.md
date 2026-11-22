# Schema

Database schema version 10 (SQLite).

## Tables

### projects
```sql
CREATE TABLE projects (
  id INTEGER PRIMARY KEY,
  key TEXT UNIQUE NOT NULL,    -- 'P-001'
  name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

### tasks
```sql
CREATE TABLE tasks (
  id INTEGER PRIMARY KEY,
  key TEXT UNIQUE NOT NULL,    -- 'T-001'
  project_id INTEGER NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'planning'
    CHECK (status IN ('planning', 'review', 'waiting-on', 'backlog', 'closed')),
  priority TEXT NOT NULL DEFAULT 'medium',
  scope TEXT,
  assignee TEXT,
  notes TEXT,
  branch TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY (project_id) REFERENCES projects(id)
);
```

### logs
```sql
CREATE TABLE logs (
  id INTEGER PRIMARY KEY,
  entity_type TEXT NOT NULL,   -- 'project' or 'task'
  entity_id INTEGER NOT NULL,
  timestamp TEXT NOT NULL,
  user TEXT NOT NULL DEFAULT 'me',
  action TEXT NOT NULL,
  message TEXT NOT NULL,
  details TEXT
);
```

### attachments
```sql
CREATE TABLE attachments (
  id INTEGER PRIMARY KEY,
  task_id INTEGER,
  project_id INTEGER,
  filename TEXT NOT NULL,
  stored_path TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES tasks(id),
  FOREIGN KEY (project_id) REFERENCES projects(id)
);
```

### links
```sql
CREATE TABLE links (
  id INTEGER PRIMARY KEY,
  task_id INTEGER,
  project_id INTEGER,
  label TEXT NOT NULL,
  uri TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES tasks(id),
  FOREIGN KEY (project_id) REFERENCES projects(id)
);
```

### pulls
```sql
CREATE TABLE pulls (
  id INTEGER PRIMARY KEY,
  task_id INTEGER NOT NULL,
  from_branch TEXT NOT NULL,
  to_branch TEXT NOT NULL,
  created_at TEXT NOT NULL,
  merged_at TEXT,
  FOREIGN KEY (task_id) REFERENCES tasks(id)
);
```

### note_diffs
```sql
CREATE TABLE note_diffs (
  id INTEGER PRIMARY KEY,
  task_id INTEGER NOT NULL,
  version INTEGER NOT NULL,
  diff TEXT NOT NULL,
  old_text TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  user TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES tasks(id)
);
```

## Enums

### Project Status
- `active`
- `inactive`

### Task Status
Board columns, left to right (schema v10):
- `planning` — scope still being worked out; default for new tasks
- `review` — back with results, a decision, or a PR to review
- `waiting-on` — out with other people or an external process
- `backlog` — parked for later
- `closed` — finished (`task complete`, `pr merge`)

Input aliases (CLI, API, TUI): `todo`/`doing` → `planning`, `follow-up` →
`review`, `blocked` → `waiting-on`, `done` → `closed`.

### Task Priority
- `low`
- `medium`
- `high`
- `critical`

### Log Actions
- `CREATE`
- `UPDATE`
- `COMMIT`
- `ATTACH`
- `LINK`
- `ARCHIVE`
- `UNARCHIVE`

### Log Users
- `cli`
- `web`
- `me`

## Indexes

```sql
CREATE INDEX idx_tasks_project_id ON tasks(project_id);
CREATE INDEX idx_logs_entity ON logs(entity_type, entity_id);
CREATE INDEX idx_attachments_task ON attachments(task_id);
CREATE INDEX idx_links_task ON links(task_id);
CREATE INDEX idx_pulls_task ON pulls(task_id);
CREATE INDEX idx_note_diffs_task ON note_diffs(task_id);
```

## Migrations

Applied versions are recorded in the `migrations` table; every open runs any
pending migrations (`lun init` in the board's directory does the same).

v9 rebuilt `tasks` with the new status CHECK and mapped existing rows:
`todo`/`doing` → `planning`, `follow-up` → `review`, `blocked` →
`waiting-on`, `done` → `closed`. Ids, notes, positions and every child row
(logs, links, attachments, PRs) are kept. A pre-v9 binary refuses to open a
v9 database (version mismatch), so upgrade every `lun` that touches a board
together.

v10 added the `note_diffs` table: on every notes change, a unified diff and
the full old text are saved so any previous version can be recovered without
replaying the diff chain.
