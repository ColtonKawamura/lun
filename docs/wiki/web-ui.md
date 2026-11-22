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
- Project title
- Task count
- Search/filter buttons

### Task List
- Cards showing task key, title, status, priority
- Click card to open details modal
- Drag-and-drop reordering

### Task Details Modal
Fields:
- Status (dropdown)
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
- `q`: Quit (close modal)
- `/`: Search (if available)

## API Endpoints

### Tasks
- `GET /api/tasks` — List tasks
- `GET /api/tasks/{key}` — Get task
- `POST /api/tasks/{key}` — Update task
- `POST /api/tasks/{key}/logs` — Add log entry
- `POST /api/tasks/{key}/archive` — Archive task
- `POST /api/tasks/{key}/unarchive` — Unarchive task

### Logs
- `GET /api/tasks/{key}/logs` — Get task logs
- `POST /api/tasks/{key}/logs` — Add task log
- `GET /api/projects/{key}/logs` — Get project logs (includes task logs)
- `POST /api/projects/{key}/logs` — Add project log

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
