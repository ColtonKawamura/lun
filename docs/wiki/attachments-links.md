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

#### Links Section
- Shows all links in task modal
- Click to open
- Label and URL displayed

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
