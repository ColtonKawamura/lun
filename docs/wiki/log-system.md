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
- `UNARCHIVE`: Unarchived (`lun task unarchive`, or Undo in the web UI)
- `DELETE`: A project or a task was permanently deleted. A project's trace goes on `P-000`; a task's trace goes on its project (or `P-000`), since their own history is deleted with them

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
    status: planning -> review
```

### Write Logs
```sh
lun log <project|task> -m "message"
```

```
$ lun log T-001 -m "Fixed the bug"
Log #12 committed to T-001: Fixed the bug
```

### Show Last N Logs (`-N`)
```sh
lun log -1              # Last log
lun log -3              # Last 3 logs
lun log -5 --oneline    # Last 5, one per line
```

### Show Nth Most Recent (`HEAD~N`)
Like git: HEAD is the latest, HEAD~1 is 2nd, HEAD~2 is 3rd, etc.

```sh
lun log HEAD~0          # Latest (same as HEAD)
lun log HEAD~1          # 2nd most recent
lun log HEAD~2          # 3rd most recent
lun log HEAD~2 --oneline
```

### One-Line Format (`--oneline`)
```sh
lun log T-001 --oneline
lun log -3 --oneline
lun log HEAD~2 --oneline
```

```
2026-11-22 02:08  me  UPDATE  PR #12 open | status: planning -> review
2026-11-22 01:35  me  COMMIT  Testing log write functionality
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

### POST /api/projects/{key}/logs
Add a log entry to a project (same as task logs).

```json
{
  "message": "Project update"
}
```

### GET /api/projects/{key}/logs
Get all log entries for a project (includes project and task logs).

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

## Reading history selectively

The log is meant to be queried in slices, so an agent can answer "what
happened on this task lately?" without reading the whole history:

```sh
lun log T-001 -3 --oneline   # 3 newest entries of one task, one line each
lun log P-001 -10            # 10 newest entries for a project (incl. its tasks)
lun log -5 --oneline         # 5 newest entries anywhere
lun log HEAD~1               # the 2nd-newest entry anywhere
```

`lun log` has no `--json` output yet; `--oneline` is the compact,
script-friendly form: `timestamp  user  ACTION  commit message | details`.

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
- Use `-N --oneline` for scripting and agents
- Math renders in web UI and TUI, not CLI (CLI shows raw text)

## Edge Cases

### CLI

| Command | Behavior |
|---------|----------|
| `lun log -0` | Shows "No logs found" |
| `lun log -100` | Shows all available logs (no error) |
| `lun log T-001 -2` | Only the 2 newest entries of T-001 |
| `lun log T-001 -0` | Shows "No logs found." |
| `lun log HEAD~0` | Same as latest log |
| `lun log HEAD~999` | Error: "HEAD~999 is beyond the log history" |
| `lun log HEAD~` | Error: "'HEAD~' is not valid HEAD~N" |
| `lun log HEAD~-1` | Error: "'HEAD~-1' is not valid HEAD~N" |
| `lun log -abc` | Error: "'-abc' is not a valid number" |
| `lun log -m "msg"` | Error: entity required before -m |
| `lun log T-999` | Error: task not found |
| `lun log T-001 -m ""` | Creates empty log (allowed) |
| `lun log T-001 -m "<script>..."` | Stored as-is (CLI shows raw) |

### Web UI

- Empty log message: Rejected (400 Bad Request)
- XSS attempts: HTML entities escaped (`<script>` → `&lt;script&gt;`)
- Math rendering: KaTeX renders `$...$` and `$$...$$` delimiters
- Very long messages: Accepted (tested up to 10000 chars)
- Unicode/emoji: Accepted and displayed correctly

### API

- `POST /api/tasks/{key}/logs` with empty message: `400 Bad Request`
- `POST /api/tasks/{key}/logs` with XSS: HTML escaped in response
- `POST /api/projects/{key}/logs`: Works same as task logs
- Non-existent task/project: `404 Not Found`
