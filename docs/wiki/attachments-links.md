# Attachments & Links

## Attachments

Store files associated with projects and tasks. Files are copied to `.lun/attachments/` and linked in the database.

### CLI Commands

#### Attach a File
```sh
lun attach <task|project> <key|title> /path/to/file
```

```sh
lun attach task T-001 ./screenshot.png
lun attach project P-001 ./roadmap.md
```

#### List Attachments
```sh
lun attach ls <task|project> <key|title>
```

```sh
$ lun attach ls task T-001
- screenshot.png (.lun/attachments/screenshot.png)
- design.pdf (.lun/attachments/design.pdf)
```

#### Open Attachment
```sh
lun attach open <task|project> <key|title> <filename|id>
```

Opens with macOS default app.

```sh
lun attach open task T-001 screenshot.png
lun attach open task T-001 123
```

#### Remove Attachment
```sh
lun attach rm <task|project> <key|title> <filename|id>
```

```sh
lun attach rm task T-001 screenshot.png
```

### Web UI

#### Drag and Drop
- Drag files onto task modal
- Automatically uploads and attaches

#### Upload Button
- Click "Upload" in attachments section
- Select files from file picker

#### View
- Thumbnails for images
- File name and size
- Click to open/download

### API

#### POST /api/tasks/{key}/attachments
Upload (multipart/form-data).

```sh
curl -F "file=@screenshot.png" http://localhost:9120/api/tasks/T-001/attachments
```

Response:
```json
[
  {
    "id": 123,
    "task_id": 1,
    "filename": "screenshot.png",
    "stored_path": ".lun/attachments/screenshot.png",
    "created_at": "2026-11-22T01:35:00Z"
  }
]
```

#### GET /api/tasks/{key}/attachments
List attachments.

```json
[
  {
    "id": 123,
    "task_id": 1,
    "filename": "screenshot.png",
    "stored_path": ".lun/attachments/screenshot.png",
    "created_at": "2026-11-22T01:35:00Z"
  }
]
```

#### GET /api/attach/{id}
Download attachment.

```sh
curl -O http://localhost:9120/api/attach/123
```

### Supported Types
- Images: PNG, JPG, GIF, WebP, SVG
- Documents: PDF, TXT, MD, DOC
- Code: JS, TS, PY, RUST
- Archives: ZIP, TAR.GZ

### Storage
- Location: `.lun/attachments/`
- Naming: Original filename with `-N` suffix on collision
- Size: No limit (but affects DB size)

## Links

Record URLs associated with projects and tasks.

### CLI Commands

#### Add Link
```sh
lun link <task|project> <key|title> "<label>" "<uri>"
```

```sh
lun link task T-001 "Design" "https://figma.com/file/abc"
lun link project P-001 "Spec" "https://docs.google.com/..."
```

#### Remove Link
```sh
lun unlink <task|project> <key|title> <label|uri> [--all]
```

Matches the label or uri exactly. Several matches are refused unless you pass `--all` or the uri. Undo with `lun link restore`.

```sh
lun unlink task T-001 "Design"
lun unlink task T-001 "chat session" --all
```

#### Delete Link by id
```sh
lun link delete <task|project> <key|title> <id>
```

Deletes one link by its numeric id. No lun command prints ids: read them with `sqlite3 -readonly .lun/lun.db "select id,uri from links"`. Prefer `unlink`, which needs no id.

#### Restore Link
```sh
lun link restore <task|project> <key|title>
```

Restores the most recently deleted link of the card (one per run, newest first).

#### Open Link
```sh
lun open-link <task|project> <key|title> <label>
```

Opens with macOS `open`.

```sh
lun open-link task T-001 Design
```

#### Open URI
```sh
lun open-uri <uri> [--on <task|project> <key|title>]
```

```sh
lun open-uri "https://example.com"
lun open-uri "https://example.com" --on task T-001
```

### Web UI

#### Dragging a link onto a card
Drag a URL from a browser tab, or a link from any app that puts a URL on the drag, onto a card on the board. The card highlights with "drop to add link" and the link is saved on that task. The same works on the Links section of the open task window — even while that section is collapsed, where the drop opens it so the new link is visible.

**Mail messages go through a file.** Safari does not hand a web page a drag straight from Apple Mail (it never drops), but it does take the message file. So: drag the message from Mail to the Desktop (Mail saves it as a `.eml` file), then drag that file onto a card or into the Links section. The board reads the message's Message-ID and Subject from the file:
- The link is stored as `message:%3C<message-id>%3E`, the same form Mail puts on its own drags, so the chip opens that message in Mail
- The subject becomes the label
- A message already linked to the task is not added twice
- A `.eml` with no Message-ID is reported, not stored

Any other file dropped on a card is refused with a message and not attached; use the Attachments section for files. `file://` drags are ignored, and a `file:` path is never stored: it means nothing on a phone.

The first link on a task shows as a chip under its title. Click or tap the chip to open it: `http(s)` in a new tab, every other scheme (`message:`, `hermes:`, `obsidian:`) handed to the OS. `Ctrl+L` on the board opens the same link. Extra links show `+N` on the chip and are listed in the modal.

#### Links Section
- Starts collapsed: `Enter` opens it and `Esc` closes it (`j`/`k` to reach the header); on a phone tap the header row. The header shows the link count while closed, and dropping a link into it opens the section
- Shows all links in the task modal; click one to open it (`Ctrl+L` opens the selected one)
- Label and URL displayed; the label is the mail subject when the link came from Mail
- The row under the list adds a link by hand (label + link). This is how a link gets onto a task from an iPhone, where no app can be dragged into Safari.
- `dd` on the section deletes the selected link (Undo: `u`)

### API

#### GET /api/tasks/{key}/links
List links.

```json
[
  {
    "id": 123,
    "task_id": 1,
    "label": "Design",
    "uri": "https://figma.com/file/abc",
    "created_at": "2026-11-22T01:35:00Z"
  }
]
```

#### POST /api/tasks/{key}/links
Add a link. Body: `{"label": "Design", "uri": "https://figma.com/file/abc"}`. An empty label falls back to the uri. Returns the new link with `201`; `400` when `uri` is empty, `404` for an unknown task.

```sh
curl -XPOST http://localhost:9120/api/tasks/T-001/links \
  -H 'Content-Type: application/json' \
  -d '{"label": "Mail", "uri": "message:%3Cabc123@example.com%3E"}'
```

#### GET /api/links
Every task link on the board, oldest first, each entry carrying the task's `task_key` as well as the link's `id`, `label`, `uri` and `created_at`. One request backs the link chip on every card.

### TUI

Inside TUI task view:
- Links section shows all links
- Click or press `Enter` to open
- `lun open-link` command also works

### Examples

#### Multiple Links
```sh
lun link task T-001 "Design" "https://figma.com/..."
lun link task T-001 "Spec" "https://docs.google.com/..."
lun link task T-001 "Bug" "https://github.com/..."
```

#### Project Links
```sh
lun link project P-001 "Roadmap" "https://roadmap.com/..."
lun open-link project P-001 Roadmap
```

## Tips

- Use quotes for labels with spaces: `"My Label"`
- Attachments are copied, not symlinked
- Links can be opened with `lun open-link` or `lun open-uri`
- Web UI shows thumbnails for images
- Math in attachment filenames: `lun attach task T-001 "$F=ma$.png"`
