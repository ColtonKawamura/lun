//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 2: SQLite data layer behind `lun init`. The full CLI (`status`,
//! `log`, ...) lands in Phase 3.

pub mod db;

pub use db::{Lun, LinkTarget, Project, ProjectSpec, Task, TaskSpec};
