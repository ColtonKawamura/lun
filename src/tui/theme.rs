//! Purple theme per docs/plan.md Phase 5: dark navy background, bright
//! purple primary accent, magenta separators, cyan command names, dim
//! hints, lavender IDs, and the per-status colors.

use ratatui::style::{Color, Style};

/// Dark navy / near-black background.
pub const BG: Color = Color::Rgb(13, 17, 28);
/// Bright purple — banner, section headers, prompt symbol.
pub const PURPLE: Color = Color::Rgb(177, 121, 255);
/// Magenta / pinkish-purple — separators and underlines.
pub const MAGENTA: Color = Color::Rgb(221, 90, 221);
/// Cyan / light blue — command names.
pub const CYAN: Color = Color::Rgb(100, 200, 255);
/// Dim gray / muted white — hints and descriptions.
pub const DIM: Color = Color::Rgb(128, 133, 150);
/// Soft lavender — IDs (T-00N, P-00N).
pub const LAVENDER: Color = Color::Rgb(190, 170, 240);
/// Default text.
pub const TEXT: Color = Color::Rgb(220, 220, 235);
/// Bright green — success and `done` tasks.
pub const DONE: Color = Color::Rgb(60, 220, 120);
/// Bright red — errors.
pub const ERROR: Color = Color::Rgb(255, 80, 80);

/// `todo` = blue, `in-progress` = bright purple, `review` = magenta,
/// `done` = bright green.
pub fn status_color(status: &str) -> Color {
    match status {
        "todo" => Color::Rgb(80, 140, 255),
        "in-progress" => PURPLE,
        "review" => MAGENTA,
        "done" => DONE,
        _ => TEXT,
    }
}

/// Style for a status word in any view.
pub fn status_style(status: &str) -> Style {
    Style::default().fg(status_color(status))
}

/// Section heading: ALL CAPS bold purple with a magenta underline row.
pub fn heading_style() -> Style {
    Style::default()
        .fg(PURPLE)
        .add_modifier(ratatui::style::Modifier::BOLD)
}

/// The selected palette row: inverted (bright purple background).
pub fn selected_style() -> Style {
    Style::default().bg(PURPLE).fg(BG)
}

/// The purple prompt symbol (`›`) used on the hint bar and palette prompt.
pub const PROMPT: &str = "\u{203a}";
