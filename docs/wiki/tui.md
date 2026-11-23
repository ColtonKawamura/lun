# TUI (Terminal UI)

The TUI provides a full-screen terminal interface with vim-like navigation.

## Launch

```sh
lun                         # From .lun/ directory
```

Or from nvim:
```sh
:Lun                        # Launch TUI
```

## Navigation

### Card View
- `j` / `ArrowDown`: Next card
- `k` / `ArrowUp`: Previous card
- `Enter`: Open card
- `Esc`: Close modal

### Modal Fields
- `j` / `ArrowDown`: Next field
- `k` / `ArrowUp`: Previous field
- `i`: Insert mode (focus input)
- `Esc`: Exit insert mode

### Global
- `?`: Toggle help
- `q`: Quit (close modal)
- `/`: Command mode

## Command Mode

Press `/` or `:` and type commands:

### Status
```sh
status                      # All projects
status P-001                # Project P-001
status P-001 --board        # Kanban board
```

### Tasks
```sh
task T-001                  # View task
task T-001 --status review  # Update status (planning|review|waiting-on|backlog|closed)
task edit T-001 --title "New title"
task complete T-001
task reopen T-001
task archive T-001
```

### Logs
```sh
log T-001                   # View logs
log T-001 -m "Fixed bug"    # Write log
```

### Add
```sh
add task "My task" proj "My project"
add proj "My project"
```

### Delete a project
```sh
rm proj "My project"                       # asks  Delete project ... [y/N]  then a commit message
rm proj P-7dqs --move-to "Other project"   # a project with tasks needs --move-to or --cascade
rm proj P-7dqs --cascade --yes             # delete its tasks too; --yes skips the y/N
```
It runs the same code as `lun rm proj` (see the CLI reference). Answering anything but `y` cancels without asking for a commit message. A project with tasks is refused before any prompt, with the exact command to rerun. `P-000 Unassigned` cannot be deleted. In the project view (shown after you create a project in the form), `D` fills in `rm proj <key>` for the highlighted project.

### Attachments
```sh
attach task T-001 ./file.png
attach ls task T-001
attach open task T-001 file.png
attach rm task T-001 file.png
```

### Links
```sh
link task T-001 "Design" "https://figma.com/..."
open-link task T-001 Design
open-uri "https://example.com"
```

### PRs
```sh
pr new T-001 --from feature --to main
pr show T-001
pr ls
pr merge T-001
```

### Search
```sh
grep "bug fix"
```

## Context

Commands default to context:
- `log` → current project or task
- `add task "..."` → current project
- `task` → current task
- `status --board` → current project

## Vim Integration

### nvim Plugin
Add to `~/.config/nvim/lua/plugins/lun.lua`:

```lua
require('lun')
```

### Keybindings
- `:Lun` — Launch TUI
- `⌘⇧L` — Open links

## Tips

- Use `/` for quick commands
- `j`/`k` work in most views
- `?` shows help
- `q` closes modals
- Commands accept `--json` flag
