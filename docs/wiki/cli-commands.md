# CLI Commands

All commands can be run from the shell or inside the TUI (with `/` or `:`).

## Global Flags

- `--json`: Output in JSON format (read commands only)
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

### `lun task <task> --status <todo|doing|follow-up|blocked|done>`
Update task status.
```sh
lun task T-001 --status doing
```

### `lun task ls [filters]`
List tasks with optional filters.
```sh
lun task ls --project P-001
lun task ls --status doing
lun task ls --priority high
lun task ls --assignee alice
lun task ls --sort created_at
lun task ls --all              # Include archived
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
lun task complete T-001         # Mark as done
lun task reopen T-001           # Mark as todo
lun task archive T-001          # Archive (hide from default views)
```

### `lun move "<task>" "<project>"`
Move task to another project.
```sh
lun move T-001 "Website"
```

## Log System

### `lun log <project|task>`
View commit-style history.
```sh
lun log P-001                   # Project logs
lun log T-001                   # Task logs
lun log T-001 --oneline         # One-line format
```

### `lun log -N`
Show last N logs across all entities.
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
