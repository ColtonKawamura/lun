# Overview

`lun` is a task and project tracker that combines the best of git, GitHub, and kanban boards into a single, CLI-first tool.

It serves two users on one database:

- **Agents** use the CLI: plain commands, plain-text output, and a git-style
  history per task they can query selectively (`lun log T-014 -3 --oneline`)
  without flooding their context. See [Using lun from agents](agents.md).
- **The person** uses the web board (`lun serve`, also a phone PWA) or the
  full-screen TUI to see and edit the same tasks.

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
- Status (board columns, left to right): `planning`, `review`, `waiting-on`, `backlog`, `closed`
  - `planning`: scope still being worked out (new tasks start here)
  - `review`: back with results, a decision, or a PR to review
  - `waiting-on`: out with other people or an external process
  - `backlog`: parked for later
  - `closed`: finished
  - Old names are accepted as input: `todo`/`doing` → `planning`, `follow-up` → `review`, `blocked` → `waiting-on`, `done` → `closed`
- Priority: `low`, `medium`, `high`, `critical`
- Can have attachments, links, logs, and PRs

### Logs
- Commit-style history for projects and tasks
- Records all state changes automatically
- Can be written manually with `lun log -m "message"` or web UI
- Supports markdown and math equations (`$F=ma$`)

## Data Storage

All data is stored in a SQLite database at `.lun/lun.db`:
- Schema version: 9
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
