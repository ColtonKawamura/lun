# Overview

`lun` is a task and project tracker that combines the best of git, GitHub, and kanban boards into a single, CLI-first tool.

## Core Concepts

### Projects
- Top-level containers for tasks (e.g., "Q4 Planning", "Website Redesign")
- Identified by `P-00N` keys (auto-incremented)
- Status: `active` or `inactive`
- Can have attachments, links, and logs

### Tasks
- Individual work items under projects (e.g., "Fix login bug", "Design homepage")
- Identified by `T-00N` keys (auto-incremented)
- Fields: title, status, priority, assignee, scope, notes, labels, branch
- Status: `todo`, `doing`, `follow-up`, `blocked`, `done`
- Priority: `low`, `medium`, `high`, `critical`
- Can have attachments, links, logs, and PRs

### Logs
- Commit-style history for projects and tasks
- Records all state changes automatically
- Can be written manually with `lun log -m "message"` or web UI
- Supports markdown and math equations (`$F=ma$`)

## Data Storage

All data is stored in a SQLite database at `.lun/lun.db`:
- Schema version: 8
- WAL mode for concurrent reads/writes
- Backups via SQLite dump/restore

## Output Format

CLI output is markdown-formatted for:
- Human readability
- Pipe to `less -R` for paging
- Redirect to files for documentation
- Easy scripting with `grep`/`awk`

## Integration

- **macOS**: Native file opening (`open`), Tailscale networking
- **Git**: Optional branch management and merge commands
- **nvim**: `:Lun` TUI launcher and `⌘⇧L` link opener
- **Web**: PWA with service worker and offline support
