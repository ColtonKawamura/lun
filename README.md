# lun

A low-dependency, CLI-first, markdown-formatted task and project version-control
tracker for macOS, mixing git-style commits/logs, GitHub-style PRs/merges/issues,
and kanban boards.

lun is built for two kinds of users at once:

- **Agents drive it from the CLI.** Every operation is a plain command with
  plain-text output, so an AI agent (or a script) can file, move and annotate
  tasks with ordinary shell calls.
- **Every change is a commit.** Each task carries a git-style history, newest
  first, so an agent can look up just the slice it needs
  (`lun log T-014 -3 --oneline`) instead of loading a whole task into its
  context.
- **The person gets a UI.** The same database backs a web kanban board / phone
  PWA (`lun serve`) and a full-screen TUI, so the human sees and edits exactly
  what the agents wrote.

The canonical store is a SQLite database (`.lun/lun.db`) — tasks, projects,
attachments, and commit logs are never plain-text files. Markdown-style output
is a human-facing view rendered from the DB.

## Task statuses

Tasks move through five board columns, left to right:

| Status | Meaning |
|---|---|
| `planning` | Scope still being worked out. New tasks start here. |
| `review` | Back with results, a decision to make, or a PR to review. |
| `waiting-on` | Out with other people or an external process (email, referee, CI, cluster jobs). |
| `backlog` | Parked for later. |
| `closed` | Finished (`lun task complete`, `lun pr merge`). |

These are the stages of the Obsidian kanban lun replaced (schema v9). The
older names are still accepted as input: `todo`/`doing` → `planning`,
`follow-up` → `review`, `blocked` → `waiting-on`, `done` → `closed`.

## Status

All phases of [docs/plan.md](docs/plan.md) (1–10) are implemented:

- DB layer + core CLI (`status`, `add proj`, `add task`, `task`, `move`, `log`)
- Task workflows (`task ls|edit|complete|reopen|archive`) and project/task attachments
- Mac linking & attachments (`attach`, `link`, `open-link`, `open-uri`)
- Full-screen TUI (purple theme): `/` command palette, board/status/project
  views, task detail with vim-style keys, in-TUI logs, `/new-task`,
  `/new proj` (plus `/new-project` alias), `/move`, and focused link opening
  ([docs/tui.md](docs/tui.md))
- TUI command prompt parity with the CLI: press `/` or `:` and run the same
  `lun ...` commands without the `lun` prefix; the latest command output stays
  on screen until another command replaces it
- Drag-and-drop file-path linking in the TUI (escaped/quoted/file URI paths,
  no file copy) and notes editing with log-on-write (`e`/`i` to edit, `Esc` to save)
- PRs & git glue (`lun pr new|show|ls|merge`): GitHub-style PRs over tasks,
  with optional best-effort `git merge` when the CWD is on the target branch
- nvim plugin: `:Lun` and `⌘⇧L` to open the link under the cursor
  ([docs/nvim.md](docs/nvim.md))

## Quick start

```sh
make                      # release build -> target/release/lun
lun init                  # create .lun/lun.db in the current directory (never a parent)
lun status                # projects overview + per-project status counts
lun                       # full-screen TUI (when run in a terminal)
```

Examples:

```sh
lun add proj "my new project"
lun add task "task3" proj "my new project"
lun task "task3" --status review
lun proj "my new project" --status inactive
lun status "my new project" --board
lun attach project P-001 ./roadmap.md
lun attach open task T-003 mock.png
```

`lun --help` lists all CLI commands.

## For agents

```sh
cd ~/repos                                  # any dir under the board works (nearest parent with .lun/lun.db); never `lun init` elsewhere
lun root                                    # which board am I using?
lun status --json                           # projects + per-status counts
lun task ls --project "soft pack" --status review
lun log T-014 -3 --oneline                  # just the 3 newest history entries
lun task T-014 --status review --message "PR #12 open"   # --message = the commit line
lun link task T-014 "PR #12" "<url>" </dev/null           # close stdin: no prompt
```

Changes prompt for a commit message on stdin unless `--message` is given;
close stdin (`</dev/null`) to accept defaults. The full agent cheat sheet,
including history lookups and known gaps, is
[docs/wiki/agents.md](docs/wiki/agents.md).

Inside the TUI, press `/` or `:` and type the same commands you would run in
the shell, but without the leading `lun`. Examples:

```sh
status
status P-001 --board
task T-003
add task "My task"
attach project P-001 "./road map.md"
attach open project P-001 "road map.md"
```

TUI command defaults are context-relative:

- `log` uses the current project, or the current task when you're already in a
  task view
- `add task "..."` uses the current project when you do not pass `proj ...`
- `task` uses the current task
- `status --board` uses the current project

Interactive command prompts (commit messages, task-creation questions, and
external-file confirmation) stay inside the bottom command prompt, and the last
command result remains visible while you type the next command.

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

- **[docs/wiki/](docs/wiki/)** — Full documentation wiki
  - [Overview](docs/wiki/overview.md)
  - [Using lun from agents](docs/wiki/agents.md)
  - [Installation](docs/wiki/installation.md)
  - [CLI Commands](docs/wiki/cli-commands.md)
  - [Web UI](docs/wiki/web-ui.md)
  - [TUI](docs/wiki/tui.md)
  - [Log System](docs/wiki/log-system.md)
  - [Attachments & Links](docs/wiki/attachments-links.md)
  - [PRs](docs/wiki/prs.md)
  - [Schema](docs/wiki/schema.md)
  - [Architecture](docs/wiki/architecture.md)
  - [Shell Completion](docs/wiki/shell-completion.md)
  - [nvim Plugin](docs/wiki/nvim.md)
  - [FAQ](docs/wiki/faq.md)
- [docs/plan.md](docs/plan.md) — the phase-by-phase plan
- [docs/architecture.md](docs/architecture.md) — design notes
- [docs/schema.md](docs/schema.md) — SQLite schema (v1→v9) reference
- [docs/tui.md](docs/tui.md) — TUI views & keybindings
- [docs/nvim.md](docs/nvim.md) — the nvim plugin

## nvim

Add this repo to your runtimepath; the plugin in `lua/lun/init.lua` gives you
`:Lun` (launch the TUI) and `⌘⇧L` (open the markdown link under the cursor via
`lun open-uri` → macOS `open`). See [docs/nvim.md](docs/nvim.md).

## License

MIT — see [LICENSE](LICENSE).
