# lun

A low-dependency, CLI-first, markdown-formatted task and project version-control
tracker for macOS, mixing git-style commits/logs, GitHub-style PRs/merges/issues,
and kanban boards.

The canonical store is a SQLite database (`.lun/lun.db`) — tasks, projects,
attachments, and commit logs are never plain-text files. Markdown-style output
is a human-facing view rendered from the DB.

## Usage (stub)

```sh
lun
```

Prints the banner and exits. Full commands (`lun status`, `lun log`, ...) and
the TUI are under construction — see [docs/plan.md](docs/plan.md) for the phase
plan and [docs/architecture.md](docs/architecture.md) for design notes.

## Build

```sh
cargo build --release
```

The binary lands at `target/release/lun`.

## License

MIT — see [LICENSE](LICENSE).
