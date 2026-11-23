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
- **Markdown**: Notes and logs render markdown
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
- Click a card (or focus it and press `Enter`) to open the details modal
- Drag a card to another column to change its status; `d`/`u` (or `>`/`<`)
  shift the focused card one column right/left; `x` closes it
- The filter bar (`f`) filters by project, status, priority, scope and text
- **Deleting a task:** open a card and press **Delete task** (red, at the bottom of the task window next to Save and Close). The task is *archived*, not erased: it leaves the board and an **Undo** toast stays for 10 seconds. After that, restore it with `lun task unarchive <key>` (`lun task ls --all` lists archived tasks). A second delete replaces the toast, so Undo only restores the latest one.
- **Deleting a project:** pick a project, then press `g d` or click **Delete project** in the project title bar. A dialog shows how many tasks it has and asks what to do: move them to another project (the default) or delete them too. `Esc`, **Cancel** or a click outside closes it without changing anything. The button is hidden on "All Projects" and on `P-000 Unassigned`.

### Task Details Modal
Fields:
- Status (dropdown: Planning, Review, Waiting-On, Backlog, Closed; the
  **Close** button sets Closed)
- Priority (dropdown)
- Project (dropdown)
- Scope (text)
- Assignee (text)
- Title (text)
- Notes (Markdown textarea)

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
- `Enter`: Submit log message

### Card View
- `j` / `ArrowDown`: Next card
- `k` / `ArrowUp`: Previous card
- `Enter`: Open card
- `Esc`: Close modal

### Global
- `?`: Toggle help
- `g p`: Project picker
- `g d`: Delete the selected project (opens a confirm dialog; `Esc` cancels)
- `q`: Quit (close modal)
- `/`: Search (if available)

## API Endpoints

### Tasks
- `GET /api/tasks` — List tasks
- `GET /api/tasks/{key}` — Get task
- `POST /api/tasks/{key}` — Update task
- `POST /api/tasks/{key}/logs` — Add log entry
- `GET /api/tasks/{key}/diffs` — Get note-edit diff history
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

### PRs
- `GET /api/tasks/{key}/prs` — List PRs

### Markdown
- `POST /api/markdown/preview` — Render markdown

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
