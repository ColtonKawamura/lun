# Web UI

The web UI is a PWA with real-time updates, touch support, and offline capability.

## Starting the Server

```sh
lun serve                         # Default: 100.71.78.78:9120 (Tailscale)
lun serve 0.0.0.0:9120           # All interfaces
lun serve 127.0.0.1:8080         # Local only
```

## Features

- **Responsive**: Works on desktop, tablet, and mobile
- **Touch-friendly**: Large buttons, swipe gestures
- **Offline**: Service worker caches assets
- **Real-time**: WebSocket updates
- **Markdown**: Notes render live, Obsidian-style: every line shows rendered, and only the line you are editing shows its raw markdown. Logs render markdown
- **Math**: KaTeX renders equations (`$F=ma$`)

## UI Layout

### Header
- The `lun` logo with the board's directory next to it (`~/repos/`,
  `~/personal-board/`; full path on hover; under the logo on narrow phones).
  It comes from `GET /api/board`, so with several boards each tab says which
  one it is. The label stays hidden if that call fails.
- Project title
- Task count
- Search/filter buttons
- Controls wrap onto extra rows on narrow screens instead of scrolling
  sideways

## Several boards
Run one `lun serve` per board, each from inside that board's directory and on
its own port (for example `~/repos` on `:9120` and `~/personal-board` on
`:9121`). They share nothing: separate databases, separate processes, so one
going down never affects the other. See
[Installation](installation.md#multiple-boards).

### Board
Five columns, left to right — the task statuses:

| Column | Color | Meaning |
|---|---|---|
| Planning | blue | scope still being worked out; new cards start here |
| Review | magenta | back with results, a decision, or a PR to review |
| Waiting-On | amber | out with other people or an external process |
| Backlog | slate | parked for later |
| Closed | green | finished |

- Cards show task key, title, priority, scope, assignee and project
- A card whose task has a link shows a chip with the first link's label; click or tap it to open the link (`http(s)` in a new tab, `message:`/`hermes:`/`obsidian:` handed to the OS). Extra links show `+N`; `Ctrl+L` opens the same link from the keyboard
- **Drag a link onto a card** (a URL from a browser tab, or a link from any app) to save it on that task. A Mail message goes through its `.eml` file: save it to the Desktop from Mail, then drop that file on the card. The card highlights while you drag over it, and the drag's plain text becomes the label. See [Attachments & Links](attachments-links.md#dragging-a-link-onto-a-card)
- Click a card (or focus it and press `Enter`) to open the details modal
- Drag a card to another column to change its status; `d`/`u` (or `>`/`<`)
  shift the focused card one column right/left; `x` archives it (Undo toast);
  to set a card to Closed use the **Close** button in its window, or `d`
- The filter bar (`f`) filters by project, status, priority, scope and text
- **Deleting a task (permanent):** open a card and press **Delete task** (red, at the bottom of the task window next to Save and Close). A dialog asks you to confirm, then removes the task and its history, links, attachments and PRs for good. `Esc`, **Cancel** or a click outside keeps it. Same as `lun rm task`.
- **Archiving a task (`x`):** select a card and press `x`. The task leaves the board and an **Undo** toast stays for 10 seconds. After that, restore it with `lun task unarchive <key>` (`lun task ls --all` lists archived tasks). A second archive replaces the toast, so Undo only restores the latest one. (`x` used to mark the task closed; it now archives. Set a task to *closed* from the task window or with `d`/`u` on the board.)
- **Deleting a project:** pick a project, then press `g d` or click **Delete project** in the project title bar. A dialog shows how many tasks it has and asks what to do: move them to another project (the default), or delete them too. Choosing "delete the tasks too" shows a checkbox ("I understand N tasks will be permanently deleted...") and keeps the red button disabled, labelled "Delete project and N tasks", until you tick it; switching back to "Move" clears the tick. A project with no tasks needs no choice. `Esc`, **Cancel** or a click outside closes it without changing anything. The button is hidden on "All Projects" and on `P-000 Unassigned`.

### Task Details Modal
Fields:
- Status (dropdown: Planning, Review, Waiting-On, Backlog, Closed; the
  **Close** button sets Closed)
- Priority (dropdown)
- Project (dropdown)
- Scope (text)
- Assignee (text)
- Title (text)
- Links (the task's links, each clickable, plus a label + link row that adds one; `Ctrl+L` opens the selected link, `dd` deletes it, `u` restores the last deleted one)
- Notes (Markdown, live editor; see [Notes editor](#notes-editor))

### Notes editor
The notes render as you type, like Obsidian's live preview. Every line shows
rendered (headings, bold, lists, checkboxes, quotes, links, code, math, tables).
The one line you are editing shows its raw markdown in a text box, so the
raw text never shows on lines you are not touching.

- **Edit a line:** click it (or tap it on a phone). The caret goes where you
  clicked, mapped into the raw text. Arrow keys move between lines: `↑`/`↓` leave
  the line from its first/last visual row (a long line that wraps moves inside
  itself first), and `←`/`→` leave it from its start/end.
- **New line:** `Enter`, `Shift+Enter` or `Ctrl+Shift+Enter`. Inside a list or
  quote, `Enter` continues the marker (`- `, `1.`, `- [ ]`, `> `), and `Enter`
  on an empty item ends the list. `Shift+Enter` is always a plain line break
  (no marker).
- **Join lines:** `Backspace` at the start of a line joins it to the line above;
  `Delete` at the end pulls the line below up.
- **Undo / redo:** `Ctrl+Z` or `Cmd+Z` undoes; `Ctrl+Shift+Z`, `Cmd+Shift+Z` or
  `Ctrl+Y` redoes. A burst of typing is one undo step.
- **Save:** `Ctrl+Enter` / `Cmd+Enter` saves and closes, as before. The **Save**
  button does the same. To insert a line with a modifier, use `Shift+Enter` or
  `Ctrl+Shift+Enter`: those never save. `Esc` leaves the text box and keeps the
  modal open. Closing the modal without saving discards the changes.
- **Blocks:** a fenced code block shows its lines in monospace, with the fence
  lines still visible but muted. A `$$ ... $$` block is one unit: click any part of
  the rendered formula to edit the whole block raw. A table row is one unit; header
  and separator rows (`| a | b |` then `|---|---|`) are read as one table, and
  the rows lay out in equal-width columns.
- **Links:** a plain click on a link edits its line. `Ctrl`/`Cmd`+click opens it.
- The rendering is the same server-side renderer as the full preview, so raw
  HTML is dropped and unsafe link schemes are not turned into links.
- Limitation: a selection cannot span lines in the live editor. `Ctrl+A` in the
  notes selects only the line (or `$$` block) being edited.


### Log Input
- Located under Notes section
- Text input for log messages
- Supports markdown and math (`$F=ma$`)
- Press `Enter` or click "Log" to submit
- Navigate to with `j`/`k` (after `i` for insert mode)

### History Section
- Shows all log entries for the task
- Color-coded actions:
  - `CREATE`: Green
  - `UPDATE`: Purple
  - `COMMIT`: Orange
  - `ATTACH`: Blue
  - `LINK`: Yellow
- Timestamp, action, and message
- Math equations render via KaTeX

## Keyboard Navigation

### Modal Fields
- `j` / `ArrowDown`: Next field
- `k` / `ArrowUp`: Previous field
- `i`: Insert mode (focus input)
- `Esc`: Exit insert mode
- `Enter`: Submit log message (in the notes, `Enter` makes a new line; see [Notes editor](#notes-editor))
- `Ctrl`/`Cmd`+`Enter`: Save the task (from any field)

### Card View
- `j` / `ArrowDown`: Next card
- `k` / `ArrowUp`: Previous card
- `Enter`: Open card
- `Esc`: Close modal

### Global
- `?`: Toggle help
- `a`: New card in the selected column. The title box opens empty, and the `a` is not typed into it
- `f`: Toggle the filter bar. The `f` is not typed into the search box
- `g p`: Project picker
- `g d`: Delete the selected project (opens a confirm dialog; `Esc` cancels)
- `x`: Archive the selected card (Undo toast for 10 seconds). It no longer closes the task
- `/`: Command palette. The `/` is not typed into the palette input
- `Space c c`: Copy the selected card's task key (e.g. `T-ne64`) to the clipboard, with a short toast. Space is the leader key, so press Space, `c`, then `c` within 0.8 s. Works on the board, in the list and in the task modal (normal mode, not while a field is being edited)

## API Endpoints

### Tasks
- `GET /api/tasks` — List tasks
- `GET /api/tasks/{key}` — Get task
- `POST /api/tasks/{key}` — Update task
- `POST /api/tasks/{key}/logs` — Add log entry
- `GET /api/tasks/{key}/diffs` — Get note-edit diff history
- `POST /api/tasks/{key}/delete` — Permanently delete a task and its history/links/attachments/PRs (`lun rm task`). Returns `{deleted, log_entries, links, attachments, prs}`; `404` if the task does not exist.
- `POST /api/tasks/{key}/archive` — Archive task
- `POST /api/tasks/{key}/unarchive` — Unarchive task

### Projects
- `GET /api/projects` — List projects
- `POST /api/projects` — Create a project (`{"name": "...", "status": "active"}`)
- `GET /api/projects/{key}` — One project, with `task_count` (archived tasks included)
- `POST /api/projects/{key}/delete` — Delete a project. Body (optional): `{"move_to": "P-xxxx"}` or `{"cascade": true}`, plus an optional `"message"`. Returns `{"deleted", "tasks_moved", "tasks_deleted", "moved_to"}`.
  - `409` when the project still has tasks and neither option is given, when it is `P-000`, or when `move_to` is the project itself.
  - `404` for an unknown project or `move_to`; `400` when both options are given.

### Logs
- `GET /api/tasks/{key}/logs` — Get task logs
- `POST /api/tasks/{key}/logs` — Add task log
- `GET /api/projects/{key}/logs` — Get project logs (includes task logs)
- `POST /api/projects/{key}/logs` — Add project log

### Note Diffs
- `GET /api/tasks/{key}/diffs` — Note-edit diff history (oldest first).
  Each entry has `id`, `task_id`, `version`, `diff` (unified diff),
  `old_text` (full previous notes), `timestamp`, `user`, `log_id` and `log_hash` (the log entry that recorded
  the edit; `lun log <log_hash>` shows it). Log entries from `GET /api/tasks/{key}/logs` carry a `hash` too;
  the card back shows its first 7 characters after each message.

### Attachments
- `GET /api/tasks/{key}/attachments` — List attachments
- `POST /api/tasks/{key}/attachments` — Upload (multipart)

### Links
- `GET /api/tasks/{key}/links` — List links
- `POST /api/tasks/{key}/links` — Add a link: `{"label": "...", "uri": "message:%3C...%3E"}`. An empty label falls back to the uri. `201` with the new link; `400` when `uri` is empty
- `GET /api/links` — Every task link on the board, oldest first, each entry with its `task_key`. One request backs the link chip on every card

### PRs
- `GET /api/tasks/{key}/prs` — List PRs

### Markdown
- `POST /api/markdown/preview` — Render a whole markdown document (the full preview)
- `POST /api/markdown/lines` — Render notes units for the live editor. Body: `{"items": [{"t": "<raw text>", "k": "line|code|fence|math|thead|trow|tsep"}]}` (at most 2000 items per request). Response: `{"html": ["...", ...]}`, one inner HTML fragment per item in request order. Each item is one line, or one whole `$$ ... $$` block for `k: "math"`. The web UI uses this; a `line` item is rendered with the same pipeline as the preview, so sanitizing and math are the same.

## Service Worker

The PWA registers a service worker for:
- Offline asset caching
- Background sync
- Push notifications (future)

## Theming

CSS variables in `:root`:
```css
--bg: #1a1a2e;
--bg-secondary: #16213e;
--bg-card: #0f3460;
--accent: #6c3baa;
--accent-light: #8b5cf6;
--text: #e2e8f0;
--text-muted: #94a3b8;
--border: #334155;
--success: #22c55e;
--warning: #f59e0b;
--danger: #ef4444;
```

## Installation

1. Build: `make` or `cargo build --release`
2. Run: `lun serve [BIND]`
3. Open: `http://localhost:9120` or `http://100.71.78.78:9120`
