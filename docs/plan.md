---
categories:
  - "[[../overhead/categories/projects|projects]]"
created: 2026-09-25
tags:
  - open
---
# Lun Project Spec and Phase Plan

## 0. Overview

**Goal:**  
Create a low-dependency, CLI-first, markdown-formatted, task and project version-control tracker that mixes:

- git-style commits/logs
- GitHub-style PRs/merges/issues
- kanban boards

It may end up as a standalone CLI/TUI plus an optional nvim plugin.

**Platform:** macOS only (for now).

**Core constraints:**

- MUST have vim-style keybindings in the TUI.
- MUST be able to link to any data on the computer (with explicit opt-in).
- Tasks, projects, attachments, and commit logs MUST NOT be stored as simple plain text files as the canonical source of truth.
  - Canonical storage MUST be a database (likely SQLite for low dependency).
  - Markdown output/views are for human-facing CLI/TUI, not primary storage.
  - Intent: agents can query subsets of data without loading huge text blobs, keeping context small.
- CLI-first: invoked as `lun` in the terminal.
- Mac-only: can rely on macOS tools (e.g., `open`, drag-and-drop support, macOS key codes).

---

## 1. High-Level Architecture (for all phases)

Agents should assume this architecture unless explicitly changed:

- **Core engine:**  
  - Language: compiled, CLI-friendly (Rust or Go are good; pick one and stick to it).  
    - For TUI + vim keys, Rust + `ratatui`/`crossterm` is a good default.  
  - Provides:
    - Database initialization and migrations (SQLite in a `.lun/` folder).
    - Domain models (projects, tasks, logs, attachments, links, PRs).
    - CLI commands (`lun status`, `lun log`, etc.).
    - TUI mode (`lun` full-screen interface).
- **Database:**
  - SQLite file (e.g., `.lun/lun.db`) inside the repo or user home.
  - Tables roughly:
    - `projects`
    - `tasks`
    - `logs`
    - `attachments`
    - `links`
    - (optional) `prs`, `config`, `user_profiles`
- **Markdown views:**
  - CLI and TUI render markdown-like text to terminal, but data comes from DB.
  - Files (if any) in `.lun/` are generated views or templates, not canonical state.
- **Git integration:**
  - The repo itself is a git repo.
  - Lun maintains its own **logical commits** in the `logs` table.
  - Optional: helper commands that run `git commit` using log messages, but not mandatory.
- **NVIM integration (later phase):**
  - A small Lua plugin that:
    - Launches the TUI (`:Lun`).
    - Provides keybinding(s) to open links under cursor (`command+shift+l` on Mac).
    - Optionally sends commands to the `lun` binary via RPC or `jobstart()`.

---

# Phase Breakdown

Each phase is designed to fit within ~125k tokens of context for an agent.  

---

## Phase 1 — Repo, Tooling, and Project Skeleton

**Objective:**  
From an empty folder, create a usable repo and initial scaffolding for `lun`.

**Tasks:**

1. **Initialize git + GitHub:**
   - Create a new git repo in the current folder.
   - Add `.gitignore` suited for:
     - SQLite DB files (`.lun/*.db`)
     - build artifacts
     - logs
   - Add `README.md` with:
     - Short description of `lun`.
     - Basic usage stub (`lun` command).
   - Add a license (MIT or similar).
   - Create a new GitHub repository:
     - Origin remote set.
     - Push initial commit.

2. **Decide on implementation language & minimal dependencies:**
   - Pick **one** language:
     - Recommended: Rust + `cargo` (good for TUI & Mac).
   - Document the choice in `README.md` and a `docs/architecture.md`.

3. **Bootstrap project structure:**
   - Create:
     - `src/` (or equivalent) with:
       - `main` entry.
       - placeholder CLI argument parsing.
     - `.lun/` folder for DB and internal config.
     - `docs/` for specs.
   - Implement a stub `lun` binary that:
     - Prints a simple banner.
     - Exits cleanly.

**Deliverables:**

- Git repo with initial commit.
- GitHub remote set and pushed.
- Basic `lun` binary (even if only `println!("lun v0.0.0");`).
- Architecture notes describing language choice and DB (SQLite) intent.

---

## Phase 2 — Data Model & Database Layer

**Objective:**  
Define schema and core DB access so future phases can rely on a robust backend.

**Core requirement:**  
Tasks, projects, attachments, and logs are **stored in the DB**, not plain text.

**Tasks:**

1. **Design schema (tables & relationships):**

   At minimum:

   - `projects`  
     - `id` (integer PK)  
     - `project_key` (string like `P-001`)  
     - `name` (string)  
     - `status` (enum/string: `planning`, `active`, `in-progress`, etc.)  
     - `created_at`, `updated_at`  
   - `tasks`  
     - `id` (integer PK)  
     - `task_key` (string like `T-008`)  
     - `project_id` (FK to `projects`, nullable for Unassigned tasks)  
     - `title`  
     - `status` (`todo`, `in-progress`, `review`, `done`)  
     - `priority` (`low`, `med`, `medium`, `high`)  
     - `assignee`  
     - `branch` (string, optional)  
     - `labels` (serialized JSON or separate table)  
     - `created_at`, `updated_at`
   - `logs`  
     - `id`  
     - `entity_type` (`project`, `task`)  
     - `entity_id`  
     - `timestamp`  
     - `user` (e.g., `me`)  
     - `action` (`CREATED`, `UPDATE`, `ATTACH`, `COMMENT`, etc.)  
     - `message` (commit message)  
     - `details` (JSON with field changes, notes, etc.)
   - `attachments`  
     - `id`  
     - `task_id` (FK to tasks)  
     - `filename`  
     - `stored_path` (path inside `.lun/attachments` or similar)  
     - `created_at`
   - `links`  
     - `id`  
     - `task_id` or `project_id`  
     - `label` (e.g., `obsidian`)  
     - `uri` (e.g., `obsidian://open?...`)  
     - `created_at`

   Optional:
   - `prs` table (later phases).
   - `config` table for user preferences.

2. **Implement DB initialization and migrations:**
   - On first run (`lun init`), create:
     - `.lun/lun.db` (SQLite).
     - Schema via migration tool or hand-written SQL.
   - On subsequent runs, detect if migrations are needed and apply.

3. **Implement a data access layer:**
   - Functions/structs for:
     - Creating and listing projects.
     - Creating and listing tasks.
     - Recording log entries.
     - Adding attachments and links.
   - Guarantee that **every state change** writes a `logs` entry with a commit-like message.

4. **“Unassigned” project semantics:**
   - Create a special project:
     - `project_key = "P-000"`, `name = "Unassigned"`.
   - Any task with no explicit project is assigned to `P-000`.

**Deliverables:**

- DB schema documented in `docs/schema.md`.
- Code for DB init and migrations.
- Stable data access methods (tested with small unit tests).

---

## Phase 3 — Core CLI Commands (Non-TUI)

**Objective:**  
Implement the core CLI behaviors using the DB backend and markdown-style terminal output.

### Command behavior examples (must be preserved)

#### `lun status`

Input:

```bash
lun status
```

Output (formatted in terminal as markdown-like text):

```bash
Projects
--------

ID      Name               Status       Open  Review  Done
P-001   paper-stack        in-progress  3     1       7
P-002   granE-friction     planning     5     0       0
P-003   lun-cli            active       2     0       4
P-000   Unassigned         active       2     0       0


Tasks
-----

ID      Project        Title                                Status       Priority  Assignee  Branch
T-010   paper-stack    Tune ball–chain damping params       in-progress  high      me        feat/damping-sweep
T-011   paper-stack    Analyze restitution vs stack size    todo         medium    me
T-012   paper-stack    Write methods section draft          review       high      me        feat/methods-draft
T-020   granE-friction Design frictional pack.m pipeline    todo         high      me
T-021   granE-friction Save contact histories in pack.m     todo         medium    me
T-030   lun-cli        Implement `lun status` command       in-progress  high      me        feat/lun-status
T-031   lun-cli        Add markdown board view              done         low       me        feat/board-view
T-040   Unassigned     Sketch ideas for `lun board`         todo         medium    me
T-041   Unassigned     Refactor personal dotfiles           todo         low       me

Summary: 4 projects · 9 tasks (5 todo, 2 in-progress, 1 review, 1 done)
```

**Implementation notes:**

- “Open” = tasks with `status` in `{todo, in-progress}` (or as defined).
- “Review” = tasks with `status = review`.
- “Done” = tasks with `status = done`.
- Any task with no project is aggregated under `P-000 Unassigned`.

#### `lun status <project>`

Input:

```bash
lun status paper-stack
```

Output:

```bash
Project: paper-stack
====================

Overview
--------

ID:        P-001
Name:      paper-stack
Status:    in-progress

Tasks by Status:
- todo:         2
- in-progress:  1
- review:       1
- done:         7


Tasks
-----

ID      Title                                Status       Priority  Assignee  Branch
T-010   Tune ball–chain damping params       in-progress  high      me        feat/damping-sweep
T-011   Analyze restitution vs stack size    todo         medium    me
T-012   Write methods section draft          review       high      me        feat/methods-draft
T-001   Set up ball–chain simulation         done         high      me        feat/chain-sim
T-002   Implement restitution measurement    done         high      me        feat/restitution
T-003   Explore stack length sweep           done         medium    me
T-004   Document dimensionless parameters    done         medium    me
T-005   Validate negligible-gravity regime   done         medium    me
T-006   Prepare figures for restitution plot done         medium    me
T-007   Draft introduction section           done         low       me

Summary: 1 project · 11 tasks (2 todo, 1 in-progress, 1 review, 7 done)
```

Also allow:

```bash
lun status P-001
```

to behave identically.

#### Creating a task via project command

Flow:

```bash
lun status paper-stack
```

Then:

```bash
lun proj add task "my task title"
```

Interactive prompts:

```text
Status?:
todo

Priority?:
med

Assignee? (default: me):
me

Commit message?:
add task "my task title" to paper-stack
```

Output after creation:

```markdown
Created task T-008 in project paper-stack
Committed: add task "my task title" to paper-stack
```

**Implementation notes:**

- Create the task in DB.
- Assign it to `paper-stack` (or the selected project).
- Create a `logs` entry with:
  - `action = "CREATED"` or `add-task`.
  - `message = commit message`.
  - `details` including status and priority.
- Task IDs like `T-008` auto-increment based on DB.

#### Viewing a task

Input:

```bash
lun task "my task title"
```

Output:

```markdown
Task T-008
==========

Project:   paper-stack
Title:     my task title
Status:    todo
Priority:  med
Assignee:  me
Labels:    []
Branch:
Created:   2026-09-25 16:45
Updated:   2026-09-25 16:45

Checklist:
- [ ] (add checklist items with `lun task edit T-008`)

Notes:
- (add notes with `lun task edit T-008`)

History (log):
- 2026-09-25 16:45  me  CREATED
    Status: todo, Priority: med
    Commit: add task "my task title" to paper-stack
```

#### Logs for projects

Input:

```bash
lun log paper-stack
```

Output:

```markdown
Log: paper-stack
================

2026-09-25 16:45  me  add-task T-008 "my task title"
    Status: todo, Priority: med

2026-09-25 15:10  me  update-task T-012 "Write methods section draft"
    Status: review

2026-09-25 14:30  me  close-task T-007 "Draft introduction section"
    Status: done
```

#### Logs for tasks

Input:

```bash
lun log "my task title"
```

Output:

```markdown
Log: Task T-008 "my task title"
===============================

2026-09-25 16:45  me  CREATED
    Project: paper-stack
    Status:  todo
    Priority: med
    Commit: add task "my task title" to paper-stack

2026-09-25 17:02  me  UPDATE
    Field changes:
      Status:  todo -> in-progress
    Commit: start work on "my task title"

2026-09-25 17:30  me  UPDATE
    Field changes:
      Priority: med -> high
    Commit: raise priority for upcoming deadline

2026-09-25 18:10  me  COMMENT
    Note: "Need to check damping parameters before finalizing."

2026-09-25 19:00  me  UPDATE
    Field changes:
      Status:  in-progress -> done
    Commit: finish "my task title"
```

**Parsing rules:**

- The CLI must differentiate between **tasks** and **projects** based on exact match:
  - If a string matches a project name or project ID, treat as project.
  - If it matches a task title or task ID, treat as task.
- Names with whitespace MUST be in quotes:
  - `lun status "paper stack project"`
  - `lun log "my task title"`
- Names without whitespace (e.g., `proj-title`, `projTitle`) do not need quotes.

**Deliverables:**

- Working CLI commands (`status`, `proj add task`, `task`, `log`).
- DB-backed implementations matching the examples.
- Unit tests covering the examples.

---

## Phase 4 — Mac Linking & Attachments (CLI Level)

**Objective:**  
Allow tasks/projects to link to arbitrary data on the Mac (with opt-in), including:

- file paths
- URLs
- custom URIs like obsidian links

**Requirements:**

- Attachments and links:
  - Stored in DB (`attachments` and `links` tables).
  - Optionally stored under `.lun/attachments` for copies.
- Opt-in linking:
  - When a user adds a link or attachment, prompt to confirm linking to a file outside the repo.
  - Example: “This path is outside the current repo. Link anyway? [y/N]”

**Tasks:**

1. **Attachment CLI:**
   - Add commands like:
     - `lun attach task T-008 /path/to/file`
   - Copy file into `.lun/attachments/` (or store absolute path with consent).
   - Create DB attachment record and log entry.

2. **Links CLI:**
   - Allow embedding custom URIs into tasks:
     - Example obsidian link:

       ```markdown
       [obsidian](obsidian://open?vault=personal&file=testing%20bar-impact%20theory%20on%20ball%20bounce)
       ```

   - Provide commands like:
     - `lun link task T-008 "obsidian" "obsidian://open?vault=..."`

3. **Open-link helper (CLI):**
   - For macOS:
     - Use `open "<uri>"` to open files or apps.
   - Implement `lun open-link <task-id> <label>` or similar.

**Deliverables:**

- Attachment and link commands.
- DB integration and logs.
- Mac `open` integration tested with file and obsidian URLs.

---

## Phase 5 — TUI Skeleton (Purple Theme, No Editing Yet)

**Objective:**  
Implement the full-screen TUI invoked by `lun`, with purple/light-blue aesthetics and basic navigation.

**Reference image (do not change link):**  
![](attachments/Pasted%20image%2020260925164348.png)

**Design spec (Purple Theme):**

### Goal

- Run `lun` → full-screen TUI.
- Inside TUI, user does **not** type `lun` anymore.
- Use `/` command palette (like Hermes).
- All data comes from DB, rendered in markdown-like text.

### COLORS & STYLE

- Background: dark navy / near-black.
- Primary accent: bright purple (headers, banner, important labels).
- Secondary accent: magenta/pinkish-purple (separators/borders).
- Command names: cyan/light blue.
- Descriptions, hints: dim gray / muted white.
- IDs: soft lavender / light purple.
- Status colors:
  - `todo`: blue
  - `in-progress`: bright purple
  - `review`: magenta
  - `done`: bright green
- Errors: bright red, bold.
- Success: bright green.
- Prompt symbol (`›`): bright purple.

Typography:

- ASCII art banner for “lun” in bright purple.
- Section headings: ALL CAPS, bold purple, magenta underline.
- Monospaced tables, Hermes-style.

### Initial Screen

On `lun`:

- Top banner in bright purple ASCII art:

  ```text
      _                    _
     | |    _   _ _ __   _| | ___  _ __
     | |   | | | | '_ \ / _` |/ _ \| '_ \
     | |___| |_| | | | | (_| | (_) | | | |
     |_____\__,_|_| |_|\__,_|\___/|_| |_|
  ```

- Below banner:

  ```text
  lun v0.1.0 — CLI-first markdown task & project tracker
  -----------------------------------------------------------
  ```

- Context block:

  ```text
  Repo:      /Users/you/projects/paper-stack
  Branch:    main
  Project:   paper-stack
  Summary:   3 projects · 9 tasks (5 todo, 2 in-progress, 1 review, 1 done)
  ```

- Board preview:

  ```text
  Board (paper-stack)
  -------------------

  Todo
    T-011  Analyze restitution vs stack size
    T-008  my task title

  In Progress
    T-010  Tune ball–chain damping params

  Review
    T-012  Write methods section draft

  Done
    T-001  Set up ball–chain simulation
    T-002  Implement restitution measurement
    ...
  ```

- Bottom hint bar:

  ```text
  -----------------------------------------------------------
  ›  type "/" for commands, ":" for quick actions, "q" to quit
  ```

### Slash Command Palette

- Press `/`:

  ```text
  › /
  ```

- Commands list overlay:

  ```text
  Commands
  --------

  /status           Show global status (projects + tasks)
  /board            Show kanban board
  /project          Select or view a project
  /task             View or edit a task
  /new-task         Create a new task in current project
  /log              Show recent logs
  /config           View configuration
  /help             Show help
  /quit             Exit lun
  ```

- Filtering example:

  ```text
  › /sta

  /status           Show global status
  /status-project   Show status for selected project
  /status-task      Show status for a specific task
  ```

- Selected row uses inverted purple background.

**Deliverables (Phase 5):**

- TUI start screen with banner and context.
- Slash command palette with navigation and filtering.
- Basic views for `/status`, `/board`, `/project` using DB.

---

## Phase 6 — TUI Task View, Vim Keybindings, and Logs

**Objective:**  
Add task detail view, vim-style navigation, and log display inside TUI.

### Vim keybindings

- `ESC`: enter “normal mode”.
- In normal mode:
  - `h/j/k/l`: move cursor.
  - `/`: open command palette.
  - `q`: quit.
  - `:` reserved for quick actions later.
- Text editing for notes/checklists can use:
  - `i`: insert mode.
  - `ESC`: back to normal mode.

### Task View and Logs (TUI)

Command: `/task "my task title"` (from palette).

Display (as in CLI, but colored):

```text
Task T-008
==========

Project:   paper-stack
Title:     my task title
Status:    todo
Priority:  med
Assignee:  me

Checklist:
- [ ] (add checklist items with /edit-task)

Notes:
- Focus on CLI-first, markdown-backed design.
- Cursor here |

Attachments:
- (drag a file into the TUI to insert a link at the cursor)

History:
- 2026-09-25 16:45  CREATED
    Status: todo, Priority: med
    Commit: add task "my task title" to paper-stack

-----------------------------------------------------------
›  drag file now, or type "/" for commands
```

### “status” command in TUI (connection to other data)

While in the TUI, you can press `ESC` (normal mode) and type:

```markdown
status "my task title"
```

This should bring up the task log (same as `lun log "my task title"`):

```markdown
Log: Task T-008 "my task title"
===============================

2026-09-25 16:45  me  CREATED
    Project: paper-stack
    Status:  todo
    Priority: med
    Commit: add task "my task title" to paper-stack

2026-09-25 17:02  me  UPDATE
    Field changes:
      Status:  todo -> in-progress
    Commit: start work on "my task title"

2026-09-25 17:30  me  UPDATE
    Field changes:
      Priority: med -> high
    Commit: raise priority for upcoming deadline. note here: [obsidian](obsidian://open?vault=personal&file=testing%20bar-impact%20theory%20on%20ball%20bounce)

2026-09-25 18:10  me  COMMENT
    Note: "Need to check damping parameters before finalizing."

2026-09-25 19:00  me  UPDATE
    Field changes:
      Status:  in-progress -> done
    Commit: finish "my task title"
```

**Deliverables (Phase 6):**

- Vim-style navigation and mode handling in TUI.
- Task view screen using DB.
- Ability to invoke `status "my task title"` from within TUI to show logs.

---

## Phase 7 — Drag-and-Drop Attachments in TUI

**Objective:**  
Implement drag-and-drop so that dropping a file into the TUI:

- Saves an attachment.
- Inserts a Markdown link **at the cursor position** in the active buffer.
- Records a commit/log entry.

**Behavior example:**

You’re editing a task; cursor is in Notes:

```text
Task T-050 (edit mode)
======================

Project:   lun-cli
Title:     Design lun TUI
Status:    in-progress
Priority:  high
Assignee:  me

Checklist:
- [x] Sketch initial layout
- [ ] Add status/board shortcuts
- [ ] Integrate logs view

Notes:
- Focus on CLI-first, markdown-backed design.
- Cursor here |
```

Drag `lun-tui-mock.png` into the TUI:

```text
Dropping file: /Users/me/Desktop/lun-tui-mock.png ...
Saved as: .lun/attachments/T-050-lun-tui-mock.png
Inserted link at cursor.

Commit message? (enter to accept default: attach "lun-tui-mock.png" to T-050)
› attach mockup "lun-tui-mock.png" to T-050

Committed: attach mockup "lun-tui-mock.png" to T-050
```

Buffer updates:

```text
Notes:
- Focus on CLI-first, markdown-backed design.
- Cursor was here → [lun-tui-mock.png](.lun/attachments/T-050-lun-tui-mock.png)
```

Full task:

```text
Task T-050
==========

Project:   lun-cli
Title:     Design lun TUI
Status:    in-progress
Priority:  high
Assignee:  me

Checklist:
- [x] Sketch initial layout
- [ ] Add status/board shortcuts
- [ ] Integrate logs view

Notes:
- Focus on CLI-first, markdown-backed design.
- [lun-tui-mock.png](.lun/attachments/T-050-lun-tui-mock.png)

History:
- 2026-09-25 16:10  CREATED
- 2026-09-25 16:30  UPDATE  Status: todo -> in-progress
- 2026-09-25 16:45  ATTACH  File: lun-tui-mock.png
    Commit: attach mockup "lun-tui-mock.png" to T-050
```

**Deliverables (Phase 7):**

- Drag-drop detection in TUI on macOS.
- Attachment creation in DB and filesystem.
- Link insertion at cursor.
- Commit/log entry written for each attachment.

---

## Phase 8 — NVIM Plugin Integration & Link Opening

**Objective:**  
Integrate with nvim and support opening links under cursor on Mac.

**Requirements:**

- From within TUI, pressing `ESC` enters normal mode.
- When the user’s cursor is over a link like:

  ```markdown
  [obsidian](obsidian://open?vault=personal&file=testing%20bar-impact%20theory%20on%20ball%20bounce)
  ```

  and they press `command+shift+l` (`⌘⇧L`), it should open that file/link.

**Behavior:**

- Anywhere over the obsidian link above, pressing `command+shift+l` opens Obsidian.

**Tasks:**

1. **Nvim plugin basic:**
   - Implement Lua plugin to:
     - Launch `lun` TUI via `:Lun`.
     - Provide mapping for `command+shift+l` (Mac-specific) to:
       - Extract URI under cursor.
       - Call `open "<uri>"` or ask `lun` backend to do it.

2. **Link detection:**
   - From buffer text, parse markdown-style links:
     - `[label](uri)`
   - On keypress:
     - Identify link under cursor.
     - Use `open` on macOS to launch.

3. **Integration with lun DB (optional but nice):**
   - When link is opened, optionally log “LINK_OPENED” in task/project logs.

**Deliverables (Phase 8):**

- Nvim plugin code.
- Working `:Lun` command.
- `command+shift+l` mapping to open obsidian and other links under cursor.

---

## Phase 9 — PRs, Advanced Workflow, and Git Glue (Optional)

**Objective:**  
Extend `lun` to cover more GitHub-like flows (PRs, merges) if desired.

**Tasks:**

- Implement `prs` table and CLI/TUI commands:
  - `lun pr new T-008 --from feature/branch --to main`
  - `lun pr show PR-001`
  - `lun pr ls`
- Map `logs` entries to PR lifecycle.
- Optional: integrate with `git diff` and `git merge`.

---

## Phase 10 — Tests, Docs, and Packaging

**Objective:**  
Make `lun` reliable and easy to install/use.

**Tasks:**

- Comprehensive tests:
  - DB schema.
  - CLI commands.
  - TUI navigation.
- Documentation:
  - `README.md` usage examples.
  - `docs/` for architecture, TUI, keybindings.
- Packaging:
  - `Makefile` or `justfile` for build/install.
  - Homebrew-style instructions (future).

