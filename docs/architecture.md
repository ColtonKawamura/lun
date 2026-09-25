# lun architecture

## Language & toolchain

**Rust + cargo** (pinned for all phases). Rationale:

- First-class TUI ecosystem: `ratatui` (rendering) + `crossterm` (input),
  which also gives us clean access to terminal escape sequences for the
  vim-style keybindings the TUI requires.
- Single static binary, no runtime dependencies — fits the "low-dependency,
  CLI-first" constraint.
- `cargo` handles builds, tests, and cross-platform (macOS-first) packaging.

## Storage

- **Canonical store: SQLite** (`rusqlite`), one file at `.lun/lun.db`.
  - `.lun/` lives in the current working directory when `lun` is run (per
    project/repo), so each tracked project gets its own database.
  - DB files, journals, and attachment copies are gitignored.
- **Markdown is a view, not a source of truth.** CLI and TUI render
  markdown-like text to the terminal, but every value comes from the DB.
  This keeps agent queries small (SELECT subsets, no big text blobs).
- Tables (Phase 2 implements): `projects`, `tasks`, `logs`, `attachments`,
  `links`, plus optional `prs` and `config`. See `docs/schema.md` (Phase 2).

## Git integration

- The repo being tracked is a git repo; `lun` keeps its own *logical commits*
  in the `logs` table (every state change writes a log entry with a
  commit-style message).
- Optional helpers may invoke real `git commit` using those messages, but
  nothing in lun requires git to be present.

## TUI (Phase 5+)

- Invoked by bare `lun` once the DB exists; purple/light-blue theme,
  `/` command palette, vim-style keys in normal mode.
- Rendering via `ratatui`, input via `crossterm`; data always from the DB.

## NVIM plugin (Phase 8)

- Small Lua plugin: `:Lun` launches the TUI; `command+shift+l` opens the
  link under cursor via macOS `open`. Talks to the `lun` binary over
  `jobstart()` if needed.

## macOS-only assumptions

- `open "<uri>"` for links/files (Phase 4/8).
- Terminal drag-and-drop of files (Phase 7).
