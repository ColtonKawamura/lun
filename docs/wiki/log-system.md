# Log System

The log system provides git-style commit history for projects and tasks.

## Concepts

### Log Entries
- Record all state changes automatically
- Can be written manually via CLI or web UI
- Support markdown and math equations (`$F=ma$`)
- Timestamped and attributed to user

### Actions
- `CREATE`: Project/task created
- `UPDATE`: Field changed
- `COMMIT`: Manual log entry
- `ATTACH`: File attached
- `LINK`: URL linked
- `ARCHIVE`: Archived
- `UNARCHIVE`: Unarchived

### Users
- `cli`: Command line interface
- `web`: Web UI
- `me`: Default user (configurable)

## CLI Usage

### View Logs
```sh
lun log <project|task>
```

```
$ lun log T-001
- 2026-11-22 01:35  me  COMMIT
    Commit: Testing log write functionality

- 2026-11-21 14:17  web  UPDATE
    status: todo -> doing
```

### Write Logs
```sh
lun log <project|task> -m "message"
```

```
$ lun log T-001 -m "Fixed the bug"
Log #12 committed to T-001: Fixed the bug
```

### Math Support
```sh
lun log T-001 -m "Physics: $F=ma$"
```

Renders as:
```
- 2026-11-22 01:42  me  COMMIT
    Commit: Physics: F = ma  (rendered as KaTeX)
```

## Web UI Usage

### Log Input
- Located under Notes section in task modal
- Text input with placeholder: "Add a log message (markdown, $math$ supported)"
- Press `Enter` or click "Log" to submit
- Navigate to with `j`/`k` (after `i` for insert mode)

### History Section
- Shows all log entries below the input
- Color-coded by action
- Math equations render via KaTeX
- Auto-refreshes on new entries

## API Usage

### POST /api/tasks/{key}/logs
Add a log entry.

```json
{
  "message": "Fixed the bug"
}
```

Response:
```json
{
  "id": 12
}
```

### GET /api/tasks/{key}/logs
Get all log entries.

```json
[
  {
    "id": 12,
    "entity_type": "task",
    "entity_id": 1,
    "timestamp": "2026-11-22T01:35:00Z",
    "user": "me",
    "action": "COMMIT",
    "message": "Fixed the bug",
    "details": "{}"
  }
]
```

## Automatic Logs

The following actions automatically create log entries:
- `lun add task` → CREATE
- `lun task edit` → UPDATE
- `lun task complete` → UPDATE
- `lun move` → UPDATE
- `lun attach` → ATTACH
- `lun link` → LINK
- `lun task archive` → ARCHIVE
- `lun task reopen` → UPDATE

## TUI Usage

Inside the TUI:
```sh
/ log T-001 -m "Fixed bug"
```

Or in task view:
```
l
```
(Shows current task logs)

## JSON Output

Add `--json` flag to get machine-readable output:
```sh
lun log T-001 --json
```

## Schema

Logs table:
```sql
CREATE TABLE logs (
  id INTEGER PRIMARY KEY,
  entity_type TEXT NOT NULL,  -- 'project' or 'task'
  entity_id INTEGER NOT NULL,
  timestamp TEXT NOT NULL,
  user TEXT NOT NULL DEFAULT 'me',
  action TEXT NOT NULL,
  message TEXT NOT NULL,
  details TEXT
);
```

## Examples

### Project Logs
```sh
lun log P-001 -m "Project kickoff: $E=mc^2$"
lun log "Website" -m "Design approved"
```

### Task Logs
```sh
lun log T-001 -m "Started implementation"
lun log T-002 -m "Bug found in production"
```

### View with Filter
```sh
lun log T-001 | grep COMMIT
lun log P-001 | head -n 10
```

## Tips

- Use quotes for messages with spaces: `lun log T-001 -m "Message with spaces"`
- Math equations need dollar signs: `$F=ma$` or `$$E=mc^2$$`
- Logs are permanent and cannot be deleted
- Use `--json` for scripting
- Math renders in web UI and TUI, not CLI (CLI shows raw text)
