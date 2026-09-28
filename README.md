# lun

A low-dependency, CLI-first, markdown-formatted task and project version-control
tracker for macOS, mixing git-style commits/logs, GitHub-style PRs/merges/issues,
and kanban boards.

The canonical store is a SQLite database (`.lun/lun.db`) — tasks, projects,
attachments, and commit logs are never plain-text files. Markdown-style output
is a human-facing view rendered from the DB.

## Status

All phases of [docs/plan.md](docs/plan.md) (1–10) are implemented:

- DB layer + core CLI (`status`, `new proj`, `add task`, `task`, `move`, `log`)
- Task workflows (`task ls|edit|complete|reopen|archive`) and project/task attachments
- Mac linking & attachments (`attach`, `link`, `open-link`, `open-uri`)
- Full-screen TUI (purple theme): `/` command palette, board/status/project
  views, task detail with vim-style keys, in-TUI logs, `/new-task`,
  `/new proj` (plus `/new-project` alias), `/move`, and focused link opening
  ([docs/tui.md](docs/tui.md))
- Drag-and-drop file-path linking in the TUI (escaped/quoted/file URI paths,
  no file copy) and notes editing with log-on-write (`e`/`i` to edit, `Esc` to save)
- PRs & git glue (`lun pr new|show|ls|merge`): GitHub-style PRs over tasks,
  with optional best-effort `git merge` when the CWD is on the target branch
- nvim plugin: `:Lun` and `⌘⇧L` to open the link under the cursor
  ([docs/nvim.md](docs/nvim.md))

## Quick start

```sh
make                      # release build -> target/release/lun
lun init                  # create .lun/lun.db in the current directory
lun status                # projects overview + per-project status counts
lun                       # full-screen TUI (when run in a terminal)
```

Examples:

```sh
lun new proj "my new project"
lun add task "task3" proj "my new project"
lun task "task3" --status done
lun proj "my new project" --status inactive
lun status "my new project" --board
lun attach project P-001 ./roadmap.md
lun attach open task T-003 mock.png
```

`lun --help` lists all CLI commands.

## Install

```sh
make install              # -> ~/.local/bin/lun
make uninstall            # remove it
```

Point `PREFIX` elsewhere (`make install PREFIX=/usr/local`). The release
binary is stripped; `rusqlite` ships its own SQLite (no system library
needed). A future Homebrew tap is on the roadmap.

## Shell completion

### zsh (primary)

```sh
make install-completions
```

Then add this to `~/.zshrc`:

```sh
fpath=(~/.zsh/completions $fpath)
autoload -Uz compinit && compinit
```

`completions/_lun` uses `lun complete -- ...` and supports task/project
keys and titles (including spaces). If you want repeated Tab to cycle through
matches, add:

```sh
zstyle ':completion:*' menu select
```

### bash (best effort)

Install and source the script (if you do not keep a local checkout, copy it to a stable path first):

```sh
mkdir -p ~/.bash_completion.d
cp /path/to/lun/completions/lun.bash ~/.bash_completion.d/lun.bash
echo 'source ~/.bash_completion.d/lun.bash' >> ~/.bashrc
source ~/.bash_completion.d/lun.bash
```

To cycle candidates with repeated Tab in bash, add:

```sh
bind 'TAB:menu-complete'
```

## Docs

- [docs/plan.md](docs/plan.md) — the phase-by-phase plan
- [docs/architecture.md](docs/architecture.md) — design notes
- [docs/schema.md](docs/schema.md) — SQLite schema (v1→v5) reference
- [docs/tui.md](docs/tui.md) — TUI views & keybindings
- [docs/nvim.md](docs/nvim.md) — the nvim plugin

## nvim

Add this repo to your runtimepath; the plugin in `lua/lun/init.lua` gives you
`:Lun` (launch the TUI) and `⌘⇧L` (open the markdown link under the cursor via
`lun open-uri` → macOS `open`). See [docs/nvim.md](docs/nvim.md).

## License

MIT — see [LICENSE](LICENSE).
