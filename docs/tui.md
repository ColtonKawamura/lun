# lun TUI (Phases 5–7)

`lun` with no arguments in a terminal launches the full-screen TUI
(piped output keeps the plain banner). Purple theme, vim-style keys.

## Views

| view          | how to get there                     | what it shows |
| ------------- | ------------------------------------ | ------------- |
| Status        | launch / `/status` / `:status`       | projects + all tasks + summary |
| Board         | `/board`                             | kanban columns (todo / in-progress / review / done) for the current project |
| Project       | `/project`                           | project list; `enter` sets the current project |
| Task          | `t` (current task) / `/task <q>`     | task detail: fields, notes, attachments, links, log history, **PRs** |
| Log           | `/log <project\|task>`               | commit-style history (newest first) |
| Help          | `/help`                              | this keybinding reference |

`/new-task` and `/config` exist in the palette but are placeholders for
later phases.

## Keybindings

| key       | where           | action |
| --------- | --------------- | ------ |
| `/`       | normal          | open the command palette |
| `:`       | normal          | quick action line (`:status <project\|task>`) |
| `j` / `k` / `↑` / `↓` | normal | navigate lists (projects, tasks, board) |
| `h` / `l` / `←` / `→` | task view | move focus between summary / notes / attachments / links |
| `gg` / `G` | normal | jump to the first / last item |
| `home` / `end` | normal | jump to the first / last item |
| `page up` / `page down` | normal | move faster through longer lists |
| `enter`   | normal          | select/open (project view: set current project; task view: open focused item) |
| `t`       | normal          | open the current task's detail view |
| `o`       | task view       | open the focused note link / attachment / explicit link |
| `c`       | normal          | toggle the current task complete / reopen |
| `i` / `e` | task view       | edit the current task's notes |
| `?`       | normal          | open help |
| `esc` / `backspace` | palette / insert / statusline / detail views | close palette / statusline, back to normal mode or previous view |
| `ctrl-s`  | insert (notes)  | save the notes draft (logs `UPDATE`; default message `update notes for <task>`) |
| `q`       | normal          | quit (blocked while notes have unsaved edits) |

Notes editing: `i`/`e` in the task view enters insert mode; type
markdown; `esc` back to normal; `ctrl-s` to save. Unsaved edits block
view switches and `q` (a message tells you to `esc` first).

Palette + statusline layout: the prompt composer is always pinned to the
last row. When `/` is open, the prompt line shows `› /<query>` (with the
query in cyan) and command suggestions render immediately above the
separator, growing upward like a shell completion popup. `:` quick
actions use the same bottom prompt line (`› status <query>`).

## Drag-and-drop attachments (Phase 7)

Drop a file (or paste a path) while a task is selected: the file is
copied into `.lun/attachments/` and an `ATTACH` log entry is written.
A copy failure rolls the file back (no dangling copy without a DB
record).

Focused items in the **Task** view can also be opened directly from the
TUI: move focus to **Notes**, **Attachments**, or **Links** with `h`/`l`,
select an item with `j`/`k`, then press `o` or `enter`.

## PRs in the TUI (Phase 9)

The task view renders a **PRs** section (via the shared task-view
renderer) and every PR lifecycle event (open = `UPDATE`, merge =
`MERGE`) appears in the task's history, so PRs are fully visible from
the TUI. Creating/merging PRs is done from the CLI (`lun pr new`,
`lun pr merge`) or a later TUI slash-command phase.
