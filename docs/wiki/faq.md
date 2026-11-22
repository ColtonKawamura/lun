# FAQ

## General

### What is lun?
lun is a CLI-first task and project tracker with git-style logs, GitHub-style PRs, and kanban boards.

### Why use lun?
- CLI-first (no GUI required)
- Markdown output (human-readable)
- Git-style logs (`lun log -m "message"`)
- Web UI for quick edits
- TUI with vim-like navigation
- SQLite backend (fast, reliable)

### Is it cross-platform?
Currently macOS only. Windows/Linux support planned.

## CLI

### How do I create a task?
```sh
lun add task "My task" proj "My project"
```

### How do I update a task?
```sh
lun task edit T-001 --title "New title" --priority high
```

### How do I view logs?
```sh
lun log T-001
```

### How do I write a log?
```sh
lun log T-001 -m "Fixed the bug"
```

### How do I attach a file?
```sh
lun attach task T-001 ./file.png
```

### How do I link a URL?
```sh
lun link task T-001 "Design" "https://figma.com/..."
```

## Web UI

### How do I start the web UI?
```sh
lun serve
```

### Can I access it remotely?
Yes, with Tailscale:
```sh
lun serve 100.71.78.78:9120
```

### Does it support math?
Yes, KaTeX renders `$F=ma$` and `$$E=mc^2$$`.

## Logs

### How do logs work?
Logs record all changes automatically:
- `CREATE`: Task/project created
- `UPDATE`: Field changed
- `COMMIT`: Manual log entry
- `ATTACH`: File attached
- `LINK`: URL linked

### Can I write manual logs?
Yes:
```sh
lun log T-001 -m "My message"
```

### Can logs contain markdown?
Yes, logs support markdown and math.

## Attachments

### What files can I attach?
Any file. Images, documents, code, archives.

### Where are attachments stored?
`.lun/attachments/`

## PRs

### How do PRs work?
lun tracks PR metadata but doesn't manage git. Use `lun pr` to create/show/merge PRs, then `git` for actual operations.

## TUI

### How do I use the TUI?
Run `lun` without arguments in a terminal.

### How do I navigate?
- `j`/`k`: Up/down
- `Enter`: Open
- `Esc`: Back/close
- `?`: Help
- `i`: Insert mode
- `/`: Command mode

### Can I run commands?
Yes, press `/` or `:` and type:
```sh
status
task T-001
log T-001 -m "Fixed bug"
```

## Import

### Can I import from Obsidian?
Yes:
```sh
lun import obsidian --execute
```

### What about other tools?
Import from CSV/JSON planned.

## Troubleshooting

### Database corrupted
```sh
sqlite3 .lun/lun.db "PRAGMA integrity_check;"
```

### Server won't start
```sh
lsof -i :9120
pkill -f "lun serve"
```

### Completion not working
```sh
eval "$(lun completion zsh)"  # or bash
```
