# lun

A low-dependency, CLI-first, markdown-formatted task and project version-control
tracker for macOS, mixing git-style commits/logs, GitHub-style PRs/merges/issues,
and kanban boards.

The canonical store is a SQLite database (`.lun/lun.db`) — tasks, projects,
attachments, and commit logs are never plain-text files. Markdown-style output
is a human-facing view rendered from the DB.

## Status

Phases 1–8 of [docs/plan.md](docs/plan.md) are implemented:

- DB layer + core CLI (`status`, `proj add task`, `task`, `log`)
- Mac linking & attachments (`attach`, `link`, `open-link`, `open-uri`)
- Full-screen TUI (purple theme): `/` command palette, board/status/project
  views, task detail with vim-style keys, in-TUI logs
- Drag-and-drop file attachments into the TUI (paste a path) and notes
  editing with log-on-write (`e`/`i` to edit, `Esc` to save)
- nvim plugin: `:Lun` and `⌘⇧L` to open the link under the cursor
  ([docs/nvim.md](docs/nvim.md))

Phases 9 (PRs/git glue) and 10 (packaging/docs) remain.

## Quick start

```sh
cargo build --release        # binary lands at target/release/lun
lun init                     # create .lun/lun.db in the current directory
lun status                   # projects + tasks overview
lun                          # full-screen TUI (when run in a terminal)
```

`lun --help` lists all CLI commands. Schema reference:
[docs/schema.md](docs/schema.md).

## nvim

Add this repo to your runtimepath; the plugin in `lua/lun/init.lua` gives you
`:Lun` (launch the TUI) and `⌘⇧L` (open the markdown link under the cursor via
`lun open-uri` → macOS `open`). See [docs/nvim.md](docs/nvim.md).

## License

MIT — see [LICENSE](LICENSE).
