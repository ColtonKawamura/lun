# Architecture

## Overview

lun is a Rust application with:
- CLI interface (`clap`)
- SQLite database (`rusqlite`)
- Web server (`warp`/`actix-web`)
- Static web UI (`static/index.html`)

## Directory Structure

```
lun/
├── src/
│   ├── main.rs           # Entry point
│   ├── cli.rs            # CLI argument parsing
│   ├── server.rs         # HTTP server
│   ├── db.rs             # Database operations
│   └── tui.rs            # Terminal UI
├── static/
│   └── index.html        # Web UI (single file)
├── docs/
│   └── wiki/             # Documentation
├── Makefile
└── README.md
```

## Components

### CLI
- `clap` for argument parsing
- Markdown output
- JSON output (`--json`)
- TUI launcher (`lun`)

### Database
- SQLite with WAL mode
- Schema version 8
- Auto-increment keys (`P-001`, `T-001`)
- Concurrent reads/writes

### Web Server
- HTTP server (`warp`/`actix-web`)
- Static file serving
- API endpoints
- WebSocket (future)

### Web UI
- Single HTML file
- Vanilla JS
- KaTeX for math rendering
- Markdown preview
- Live notes editor: `POST /api/markdown/lines` renders one line (or `$$` block) at a time; the browser keeps the caret's line raw (see [Web UI](web-ui.md#notes-editor))
- Service worker (PWA)

## API Endpoints

### Board
- `GET /api/board` — Which board this server serves: `{"root": "/Users/me/repos", "display": "~/repos/"}` (`display` is `~`-relative under `$HOME`, always with a trailing slash)

### Tasks
- `GET /api/tasks` — List
- `GET /api/tasks/{key}` — Get
- `POST /api/tasks/{key}` — Update
- `POST /api/tasks/{key}/logs` — Add log
- `GET /api/projects/{key}` — One project (+ `task_count`)
- `POST /api/projects/{key}/delete` — Delete a project (`move_to` / `cascade`)
- `POST /api/tasks/{key}/delete` — Permanently delete a task
- `POST /api/tasks/{key}/archive` — Archive
- `POST /api/tasks/{key}/unarchive` — Unarchive

### Logs
- `GET /api/tasks/{key}/logs` — List

### Attachments
- `GET /api/tasks/{key}/attachments` — List
- `POST /api/tasks/{key}/attachments` — Upload

### Links
- `GET /api/tasks/{key}/links` — List

### Markdown
- `POST /api/markdown/preview` — Render
- `POST /api/markdown/lines` — Render notes units one by one for the live editor (`{"items": [{"t", "k"}]}` → `{"html": [...]}`)

## Data Flow

```
CLI/Web → API → Database → Response
```

## Security

- No authentication (local use)
- Bind to specific IP
- CORS (disabled by default)

## Performance

- SQLite for fast queries
- Indexed columns
- WAL mode for concurrency
- Minimal JS in web UI

## Future

- Authentication
- Multi-user support
- Cloud sync
- Mobile app
