# Schema

Database schema version 8 (SQLite).

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
  status TEXT NOT NULL DEFAULT 'todo',
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

## Enums

### Project Status
- `active`
- `inactive`

### Task Status
- `todo`
- `doing`
- `follow-up`
- `blocked`
- `done`

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
```

## Migrations

Schema version is stored in `schema_version` table. Run `lun init` to upgrade.
