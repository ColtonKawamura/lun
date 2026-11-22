# lun — Documentation Wiki

> CLI-first, markdown-formatted task and project version-control tracker for macOS,
> mixing git-style commits/logs, GitHub-style PRs/merges/issues, and kanban boards.

## Table of Contents

1. [Overview](overview.md)
2. [Installation](installation.md)
3. [CLI Commands](cli-commands.md)
4. [Web UI](web-ui.md)
5. [TUI (Terminal UI)](tui.md)
6. [Log System](log-system.md)
7. [Attachments & Links](attachments-links.md)
8. [PRs & Git Integration](prs.md)
9. [Schema](schema.md)
10. [Architecture](architecture.md)
11. [Shell Completion](shell-completion.md)
12. [nvim Plugin](nvim.md)
13. [FAQ](faq.md)

---

## Quick Start

```sh
make                      # release build -> target/release/lun
lun init                  # create .lun/lun.db in the current directory
lun status                # projects overview + per-project status counts
lun                       # full-screen TUI (when run in a terminal)
lun serve                 # start the web UI (default: 100.71.78.78:9120)
```

## Features

- **CLI-first**: All operations available via command line
- **Markdown output**: Human-readable, pipe-friendly
- **Git-style logs**: `lun log <project|task> [-m "message"]`
- **Web UI**: PWA with real-time updates
- **TUI**: Full-screen terminal interface with vim-like navigation
- **Attachments**: File management with drag-and-drop
- **Links**: URL tracking and opening
- **PRs**: GitHub-style PR management
- **Shell completion**: zsh and bash support

## License

MIT — see [LICENSE](../LICENSE).
