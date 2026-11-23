# CLI Commands

All commands can be run from the shell or inside the TUI (with `/` or `:`).

## Global Flags

- `--json`: JSON output — `lun status` and `lun status <query>` only (other commands reject it)
- `--message "<msg>"`: commit message for `add proj`, `proj`, `rm proj`, `move`, `task --status`, `task edit` (skips the stdin prompt)

Every change records a commit and asks for its message on stdin when
`--message` is absent (`add task` also asks for status, priority, assignee).
Scripts and agents should pass `--message` or close stdin (`</dev/null`).
lun finds the board like git finds a repo: the current directory if it holds
`.lun/lun.db`, otherwise the nearest parent that does (the search stops at
`$HOME`). `lun root` prints the board in use. See
[Using lun from agents](agents.md).
- `--help`: Print help text
- `--version`: Print version

## Core Commands

### `lun init`
Create `.lun/lun.db` in the current directory.
```sh
lun init
```

### `lun status [name|P-00N] [--board]`
Projects overview or single project details.
```sh
lun status                       # All projects
lun status P-001                 # Project P-001
lun status "My Project"          # By name
lun status P-001 --board         # Kanban board view
```

### `lun /<name|key>`
Shortcut for `lun status <name|key>`.
```sh
lun /P-001
lun /"My Project"
```

## Project Management

### `lun add proj "<name>" [--status active|inactive]`
Create a new project.
```sh
lun add proj "Q4 Planning"
lun add proj "Test" --status inactive
```

### `lun proj <project> --status <active|inactive>`
Update project status.
```sh
lun proj P-001 --status inactive
```

### `lun rm proj <name|P-xxxx> [--move-to <project> | --cascade] [--yes] [--message "<msg>"]`
Delete a project. `<name>` is the exact project name; `<P-xxxx>` is its key. `rm project` works too.

- A project with no tasks asks `[y/N]` and is deleted. Its own history, links and attachments go with it.
- A project that still has tasks (archived ones count) is **refused** until you say what happens to them:
  - `--move-to <project>` moves every task to another project first (each move is logged on the task).
  - `--cascade` deletes the tasks too, with their logs, links, attachments, PRs and note diffs. This cannot be undone.
- `--yes` (or `-y`) skips the `[y/N]` question. Closed or empty stdin counts as "no", so scripts must pass `--yes`.
- `P-000 Unassigned` can never be deleted.
- `--move-to` and `--cascade` are mutually exclusive, and `--move-to` cannot name the project being deleted.
- Attachment copies stored under `.lun/attachments/` are removed from disk. Files that were only linked by path are never touched.
- The deletion is recorded as a `DELETE` entry on `P-000` (project key, name, task counts, where the tasks went), because the deleted project's own history goes with it. Read it with `lun log P-000`.
- Exit code 2 for a refusal or a "no"; 0 on success.

```sh
lun rm proj "Old Idea"                          # empty project: asks y/N
lun rm proj P-7dqs --yes --message "dead end"   # no prompt
lun rm proj Website --move-to "Personal Site"   # keep the tasks
lun rm proj Scratch --cascade --yes             # delete the tasks too
```

## Task Management

### `lun add task "<title>" [proj "<project>"]`
Create a new task.
```sh
lun add task "Fix login bug" proj "Website"
lun add task "Design homepage"
```

### `lun task <T-00N|title>`
View task details.
```sh
lun task T-001
lun task "Fix login bug"
```

### `lun task <task> --status <planning|review|waiting-on|backlog|closed>`
Update task status. The old names still work (`todo`/`doing` → `planning`,
`follow-up` → `review`, `blocked` → `waiting-on`, `done` → `closed`).
```sh
lun task T-001 --status review --message "PR #12 open"
lun task T-001 --status waiting-on --message "waiting on referee"
```

### `lun brief [--project P] [--status S] [--scope S] [--all] [--last] [--json]`

One line per open task (closed hidden unless `--all`/`--status closed`),
ordered by status then priority: `T-014  review  high  P-002  title  [scope]`.
`--last` appends each task's newest commit message. `lun status <project> --oneline` is the same view for one project.

### `lun task <T> [--brief | --log N | --diffs] [--json] [--fields a,b]`

`--brief` omits the history section, `--log N` keeps the newest N entries,
`--json`/`--fields` give machine-readable output (see [Agents](agents.md)).
`--diffs` shows the note-edit diff history (every edit to the notes field),
useful for recovering accidentally overwritten notes.

### `lun task edit <T[,T...]> --append-notes TEXT | --scope S`

`--append-notes` adds to the notes instead of replacing them; `--scope none` clears the scope.
A comma-separated list edits several tasks (status/priority/assignee/labels/scope/project/append-notes only).
`lun task T-1,T-2 --status review` changes the status of several tasks.

### `lun task ls [filters]`
List tasks with optional filters.
```sh
lun task ls --project P-001
lun task ls --status review
lun task ls --priority high
lun task ls --assignee alice
lun task ls --sort created_at
lun task ls --all              # Include archived
lun task ls --scope physics
lun task ls --json --fields key,status   # JSON / `--fields` / `--oneline` output
```

### `lun task edit <task> [--field value]`
Edit task fields.
```sh
lun task edit T-001 --title "New title"
lun task edit T-001 --priority high
```

### `lun task complete|reopen|archive <task>`
Task lifecycle commands.
```sh
lun task complete T-001         # -> closed
lun task reopen T-001           # -> planning (or --status <status>)
lun task archive T-001          # Archive (hide from default views)
```

### `lun move "<task>" "<project>"`
Move task to another project.
```sh
lun move T-001 "Website"
```

## Log System

### `lun log <project|task> [-N] [--oneline] [--json] [--since ID]`

`--since ID` keeps only entries with a larger log id (an empty result means nothing changed); `--json` adds id and entity key.

View commit-style history, newest first. `-N` keeps only the N newest
entries, so you can look at the recent past of one task without reading all
of it.
```sh
lun log P-001                   # Project logs (includes its tasks' entries)
lun log T-001                   # Task logs
lun log T-001 --oneline         # One-line format
lun log T-001 -3 --oneline      # Only the 3 newest entries of T-001
```

### `lun log <hash> [--old-notes]`
Every commit message in a log view ends with a short hash, e.g. `edit T-013; update note (a1b2c3d)`.
Give that hash (4+ hex characters, any prefix of the full 64) to see exactly that entry. If it edited a
task's notes, the notes diff it recorded is shown too, along with the command to put the old notes back.
`--old-notes` prints only the notes as they were *before* that edit, raw, so they can be piped back in.
```sh
lun log a1b2c3d                 # The entry plus its notes diff
lun log a1b2c3d --old-notes     # Notes as they were before that edit
lun task edit T-013 --notes "$(lun log a1b2c3d --old-notes)" --message "revert notes to before a1b2c3d"
```
An ambiguous prefix is an error (use more characters). `lun log --json` includes the full `hash`.

A task edit with no `--message` is logged with what it changed: `edit T-013; update note`,
`edit T-013; update status to Review`, several joined with `; `. The same wording is used by the web UI.

### `lun log -N`
Show last N logs across all entities (`lun log -N <entity>` limits to one entity, same as `lun log <entity> -N`).
```sh
lun log -1                      # Last log
lun log -3                      # Last 3 logs
lun log -5 --oneline
```

### `lun log HEAD~N`
Show Nth most recent (HEAD is latest).
```sh
lun log HEAD~0                  # Latest
lun log HEAD~1                  # 2nd most recent
lun log HEAD~2                  # 3rd most recent
lun log HEAD~2 --oneline
```

### `lun log <project|task> -m "message"`
Write a log entry (like `git commit -m`).
```sh
lun log T-001 -m "Fixed the bug"
lun log P-001 -m "Project kickoff: $F=ma$"
```

## Attachments

### `lun attach <task|project> <key|title> /path/to/file`
Attach a file.
```sh
lun attach task T-001 ./screenshot.png
lun attach project P-001 ./roadmap.md
```

### `lun attach ls <task|project> <key|title>`
List attachments.
```sh
lun attach ls task T-001
```

### `lun attach open|rm <task|project> <key|title> <filename|id>`
Open or remove attachments.
```sh
lun attach open task T-001 screenshot.png
lun attach rm task T-001 123
```

## Links

### `lun link <task|project> <key|title> "<label>" "<uri>"`
Record a link.
```sh
lun link task T-001 "Design" "https://figma.com/..."
```

### `lun open-link <task|project> <key|title> <label>`
Open a link via macOS `open`.
```sh
lun open-link task T-001 Design
```

### `lun open-uri <uri> [--on <task|project> <key|title>]`
Open any URI.
```sh
lun open-uri "https://example.com"
lun open-uri "https://example.com" --on task T-001
```

## PR Management

### `lun pr new <T-00N|title> [--from <branch>] [--to <branch>]`
Create a PR.
```sh
lun pr new T-001 --from feature --to main
```

### `lun pr show <PR-00N|task>`
View PR details.
```sh
lun pr show PR-001
lun pr show T-001
```

### `lun pr ls`
List PRs.
```sh
lun pr ls
```

### `lun pr merge <PR-00N|task>`
Merge a PR.
```sh
lun pr merge PR-001
```

## Search

### `lun grep "<text>"`
Search across projects, tasks, and logs.
```sh
lun grep "bug fix"
lun grep "$F=ma$"
```

## Web Server

### `lun serve [BIND]`
Start the web UI.
```sh
lun serve                         # Default: 100.71.78.78:9120
lun serve 0.0.0.0:9120           # All interfaces
lun serve 127.0.0.1:8080         # Local only
```

## Import

### `lun import obsidian [--vault PATH] [--dry-run]`
Import tasks from Obsidian vault.
```sh
lun import obsidian --dry-run     # Preview
lun import obsidian --execute     # Import
lun import obsidian --vault ~/vault  # Custom path
```

## TUI Commands

Inside the TUI, press `/` or `:` and type commands without the `lun` prefix:
```sh
status
status P-001 --board
task T-001
add task "My task"
log T-001 -m "Fixed bug"
```

Context defaults:
- `log` → current project or task
- `add task "..."` → current project
- `task` → current task
- `status --board` → current project
