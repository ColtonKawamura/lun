//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 3: SQLite data layer (Phase 2) + core CLI commands (Phase 3).
//! Phase 4: Mac linking & attachments commands.
//! Phase 5: full-screen TUI (purple theme) behind bare `lun` on a TTY.

pub mod cli;
pub mod db;
pub mod tui;

pub use db::{
    Attachment, Link, LinkTarget, LogEntry, Lun, Pr, PrSpec, Project, ProjectSpec, Task, TaskSpec,
};
