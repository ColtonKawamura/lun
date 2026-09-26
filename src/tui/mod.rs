//! Phase 5: full-screen TUI skeleton (purple theme, read-only).
//!
//! `lun` (no subcommand) launches the TUI when stdout is a TTY; piped
//! output keeps the plain banner. Rendering is split into pure functions
//! that write into a `ratatui::buffer::Buffer` (headlessly testable in
//! tests/phase5.rs), while [`run_tui`] owns the crossterm event loop.
//!
//! Views: initial screen (banner, context, board preview, hint bar),
//! slash command palette (`/`), `/status`, `/board`, `/project`, `/help`,
//! and placeholders for `/task`, `/new-task`, `/log`, `/config` (later
//! phases). No editing in Phase 5.

pub mod app;
pub mod data;
pub mod render;
pub mod term;
pub mod theme;
