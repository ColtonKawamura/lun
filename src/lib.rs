//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 3: SQLite data layer (Phase 2) + core CLI commands (Phase 3).
//! Phase 4: Mac linking & attachments commands.
//! Phase 5: full-screen TUI (purple theme) behind bare `lun` on a TTY.
//! Phase 10: Web server (lun serve) with PWA + markdown rendering.

pub mod cli;
pub mod db;
pub mod root;
pub mod server;
pub mod tui;

pub use db::{
    Attachment, AttachmentTarget, Link, LinkTarget, LogEntry, Lun, NoteDiff, Pr, PrSpec, Project,
    ProjectDeleteMode, ProjectDeleted, ProjectSpec, Task, TaskListSpec, TaskSort, TaskSpec, TaskUpdateSpec,
};
