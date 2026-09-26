//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 3: SQLite data layer (Phase 2) + core CLI commands (Phase 3).

pub mod cli;
pub mod db;

pub use db::{
    Attachment, Link, LinkTarget, LogEntry, Lun, Project, ProjectSpec, Task, TaskSpec,
};
