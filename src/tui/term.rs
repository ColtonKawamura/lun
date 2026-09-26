//! Phase 5: terminal driver for the TUI.
//!
//! Owns the two things that cannot be tested headlessly: raw mode +
//! alternate screen setup/teardown and the crossterm event loop. All
//! decisions (which key does what) live in [`handle_key`], which is a
//! pure state transition on `App` — tests/phase5.rs exercise the same
//! transitions by driving `App` directly and rendering headlessly with
//! `render::paint`.
//!
//! `launch` is what `main` calls when stdout is a TTY; piped output
//! keeps the plain banner (see src/main.rs).

use std::io::stdout;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::terminal::Terminal;

use crate::db::Lun;

use super::app::{App, View};
use super::{data, render};

/// Run the TUI against `.lun/lun.db` in `root`. Restores the terminal on
/// the way out; returns the process exit code.
pub fn launch(root: &Path, version: &str) -> Result<i32, String> {
    let lun = Lun::open(root).map_err(|e| e.to_string())?;
    let data = data::load(
        &lun,
        version,
        &root.display().to_string(),
        git_branch(root),
        None,
    )
    .map_err(|e| e.to_string())?;
    let mut app = App::new(data);

    let mut terminal =
        Terminal::new(CrosstermBackend::new(stdout())).map_err(|e| e.to_string())?;

    enable_raw_mode().map_err(|e| e.to_string())?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)
        .map_err(|e| e.to_string())?;

    let code = run_loop(&mut terminal, &mut app);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    let _ = terminal.flush();
    Ok(code)
}

/// The event loop: poll with a short timeout, dispatch, repaint.
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> i32 {
    let repaint = |terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
                   app: &App|
     -> Result<(), std::io::Error> {
        terminal
            .draw(|f| {
                let size = f.size();
                render::paint(f.buffer_mut(), size, app);
            })
            .map(|_| ())
    };

    if repaint(terminal, app).is_err() {
        return 1;
    }

    while !app.quit {
        let has_event = event::poll(Duration::from_millis(50)).unwrap_or(false);
        if has_event {
            match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    handle_key(app, &key);
                }
                // Resize and everything else: the next repaint picks up
                // the new size.
                Ok(_) => {}
                Err(_) => return 1,
            }
        }
        if !app.quit && repaint(terminal, app).is_err() {
            return 1;
        }
    }
    0
}

/// Pure key dispatch — the whole keyboard map of the TUI in one place.
///
/// Palette (open): printable chars filter, j/k/arrows move, Enter runs,
/// Esc closes. Outside the palette: `/` opens the palette, `q` quits, and
/// in the project view j/k/Enter navigate and select.
pub fn handle_key(app: &mut App, key: &KeyEvent) {
    if app.palette_open {
        match key.code {
            KeyCode::Esc => app.palette_open = false,
            KeyCode::Enter => app.run_command(app.palette_selected),
            KeyCode::Up | KeyCode::Char('k') if key.modifiers == KeyModifiers::NONE => {
                app.palette_up()
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers == KeyModifiers::NONE => {
                app.palette_down()
            }
            KeyCode::Backspace => app.palette_backspace(),
            KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE => {
                app.palette_type(c)
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => app.open_palette(),
        KeyCode::Char('q') if key.modifiers == KeyModifiers::NONE => app.quit = true,
        KeyCode::Char('j') if app.view == View::Project && key.modifiers == KeyModifiers::NONE => {
            app.project_nav(1, false)
        }
        KeyCode::Char('k') if app.view == View::Project && key.modifiers == KeyModifiers::NONE => {
            app.project_nav(-1, false)
        }
        KeyCode::Enter if app.view == View::Project => app.project_nav(0, true),
        _ => {}
    }
}

/// Current git branch of `root`, or `<none>` outside a git repo / on
/// failure (the context block shows it verbatim).
pub fn git_branch(root: &Path) -> String {
    let ok = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "branch", "--show-current"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    match ok {
        Some(b) => b,
        None => "<none>".to_string(),
    }
}
