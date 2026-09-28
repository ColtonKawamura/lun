# lun TUI (Phases 5–7)

`lun` with no arguments in a terminal launches the full-screen TUI
(piped output keeps the plain banner). Purple theme, vim-style keys.

## Command prompt parity

Press `/` or `:` to open the bottom command prompt. Inside that prompt, type
the same commands you would run in the shell, but **without** the leading
`lun`.

Examples:

```sh
status
status P-001 --board
task T-003
add task "My task" proj "My project"
task edit T-003 --notes "Some notes"
attach task T-003 "./design doc.pdf"
attach open task T-003 "design doc.pdf"
attach project P-001 ./roadmap.md
attach open project P-001 roadmap.md
link task T-003 "Design" "file:///absolute/path/design.pdf"
open-link task T-003 "Design"
log
```

The last command result stays visible above the prompt while you type the next
command. `Esc`, an empty submission, redraws, and resize keep the current
result; running another command intentionally replaces it. Long command output
can be scrolled with the normal navigation keys (`j`/`k`, arrows, PageUp,
PageDown, `gg`, `G`, Home, End).

Interactive CLI prompts stay inside the TUI command line too: task-creation
questions, commit messages, and out-of-repo attach confirmation are asked on
the same bottom prompt instead of reading raw stdin.

### Relative defaults in the TUI

TUI command defaults are relative to the current context:

- `log` uses the current task in the task view, otherwise the current project
- `task` uses the current task
- `status --board` uses the current project
- `add task "..."` uses the current project when no `proj ...` is passed

The older slash-driven conveniences still work through the same prompt and keep
their context-relative behavior.

## Views

| view          | how to get there                     | what it shows |
| ------------- | ------------------------------------ | ------------- |
| Status        | launch                               | projects + status counts + summary |
| Board         | `/board`                             | kanban columns (todo / doing / follow-up / blocked / done) for the current project |
| Project       | `/project`                           | project list; `enter` sets the current project |
| Task          | `t` (current task) / `/task <q>`     | task detail: fields, notes, attachments, links, log history, **PRs** |
| New Task      | `/new-task`                          | form to create a task (title, project, status, priority, assignee, branch, labels) |
| New Project   | `/new proj` (or `/new-project`)      | form to create a project in the current workspace |
| Move Task     | `/move`                              | form to move the current task between projects (including `Unassigned`) |
| Log           | `log <project\|task>`                | commit-style history (newest first) |
| Help          | `/help`                              | this keybinding reference |

`/config` is still a placeholder for a later phase.

## Keybindings

| key       | where           | action |
| --------- | --------------- | ------ |
| `/`       | normal          | open the CLI-equivalent command prompt |
| `:`       | normal          | open the same command prompt |
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
| `tab`     | command prompt  | apply the selected completion suggestion |
| `esc`     | command prompt  | after typing, enter vim prompt-nav mode (`j`/`k` cycle suggestions); press `esc` again to close |
| `backspace` | command prompt / insert / detail views | edit the line, delete notes chars, or go back |
| `<space> f f` | normal | open task/project finder prompt (`status ` prefilled) |
| `<space> f g` | normal | open log finder prompt (`log ` prefilled) |
| `ctrl-s`  | insert (notes)  | save the notes draft (logs `UPDATE`; default message `update notes for <task>`) |
| `q`       | normal          | quit (blocked while notes have unsaved edits) |

Notes editing: `i`/`e` in the task view enters insert mode; type
markdown; `esc` back to normal; `ctrl-s` to save. Unsaved edits block
view switches and `q` (a message tells you to `esc` first).

Command prompt layout: the prompt composer is always pinned to the last row.
When `/` or `:` is open, the prompt line stays at the bottom and completion
suggestions render immediately above the separator, growing upward like a shell
completion popup. The most recent command result remains visible behind the
prompt until another command replaces it.

## Drag-and-drop attachments (Phase 7)

Drop a file (or paste a path) while a task is selected: the file is
linked (not copied) as markdown in task notes, using `file:///absolute/path`
URIs that work with `lun open-uri` / the nvim `⌘⇧L` flow.

Accepted drop/paste formats include:
- shell-escaped paths (`/Users/me/My\ File.pdf`)
- quoted paths (`"/Users/me/My File.pdf"`)
- `file://` URIs (`file:///Users/me/My%20File.pdf`)
- multiple paths in one paste

Focused items in the **Task** view can also be opened directly from the
TUI: move focus to **Notes**, **Attachments**, or **Links** with `h`/`l`,
select an item with `j`/`k`, then press `o` or `enter`.

## PRs in the TUI (Phase 9)

The task view renders a **PRs** section (via the shared task-view
renderer) and every PR lifecycle event (open = `UPDATE`, merge =
`MERGE`) appears in the task's history, so PRs are fully visible from
the TUI. Creating/merging PRs is done from the CLI (`lun pr new`,
`lun pr merge`) or from the TUI command prompt with the same command text
minus the `lun` prefix.
