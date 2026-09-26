//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 3: SQLite data layer (Phase 2) + core CLI commands (Phase 3).

pub mod cli;
pub mod db;

pub use db::{Lun, LinkTarget, LogEntry, Project, ProjectSpec, Task, TaskSpec};
