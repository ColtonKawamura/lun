# lun — Documentation Wiki

> CLI-first, markdown-formatted task and project version-control tracker for macOS,
> mixing git-style commits/logs, GitHub-style PRs/merges/issues, and kanban boards.
>
> Agents drive it from the CLI and look up just the history they need; the
> person works from the web board or TUI on the same data. See
> [Using lun from agents](agents.md).

## Table of Contents

1. [Overview](overview.md)
2. [Using lun from agents](agents.md)
3. [Installation](installation.md)
4. [CLI Commands](cli-commands.md)
5. [Web UI](web-ui.md)
6. [TUI (Terminal UI)](tui.md)
7. [Log System](log-system.md)
8. [Attachments & Links](attachments-links.md)
9. [PRs & Git Integration](prs.md)
10. [Schema](schema.md)
11. [Architecture](architecture.md)
12. [Shell Completion](shell-completion.md)
13. [nvim Plugin](nvim.md)
14. [FAQ](faq.md)

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
- **Git-style logs**: `lun log <project|task> [-N] [--oneline] [-m "message"]` — pull only the history you need
- **Obsidian-style stages**: `planning` → `review` → `waiting-on` → `backlog` → `closed`
- **Web UI**: PWA with real-time updates
- **TUI**: Full-screen terminal interface with vim-like navigation
- **Attachments**: File management with drag-and-drop
- **Links**: URL tracking and opening
- **PRs**: GitHub-style PR management
- **Shell completion**: zsh and bash support

## License

MIT — see [LICENSE](../LICENSE).
