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

use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::terminal::Terminal;

use crate::db::Lun;

use super::app::{App, View};
use super::{data, render};

fn accepts_text_input(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(c)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            Some(c)
        }
        _ => None,
    }
}

fn accepts_shifted_shortcut(key: &KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT
}

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
    let mut app = App::with_store(data, root.to_path_buf(), lun);

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout())).map_err(|e| e.to_string())?;

    enable_raw_mode().map_err(|e| e.to_string())?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableBracketedPaste
    )
    .map_err(|e| e.to_string())?;

    let code = run_loop(&mut terminal, &mut app);

    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )
    .ok();
    let _ = terminal.flush();
    Ok(code)
}

/// The event loop: poll with a short timeout, dispatch, repaint.
fn run_loop(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, app: &mut App) -> i32 {
    let repaint = |terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
                   app: &mut App|
     -> Result<(), std::io::Error> {
        terminal
            .draw(|f| {
                let size = f.size();
                app.update_layout_metrics(size.height);
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
                // Phase 7: drag-and-drop. macOS terminals (and most
                // others) deliver a dropped file as a bracketed paste of
                // its absolute path — this is the drop event.
                Ok(Event::Paste(text)) => {
                    app.attach_dropped_file(&text);
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
/// Palette (open): printable chars filter, arrows move, Enter runs; Esc
/// enters vim-style prompt navigation (j/k cycle suggestions), Esc again
/// closes. Statusline (open): printable chars append, Backspace
/// removes, Enter runs, Esc closes. Insert mode: typing appends to the
/// notes draft, Enter is a newline, Esc returns to normal mode. Outside
/// all of that (normal mode): `/` opens the palette, `:` opens the
/// statusline, `<space> f f` opens finder (`status `), `<space> f g`
/// opens log finder (`log `), `q` quits, `t` opens the current task, and in the
/// project view j/k/Enter navigate and select.
pub fn handle_key(app: &mut App, key: &KeyEvent) {
    if app.form().is_some() {
        match key.code {
            KeyCode::Esc => app.cancel_form(),
            KeyCode::Up => app.form_nav(-1),
            KeyCode::Down | KeyCode::Tab => app.form_nav(1),
            KeyCode::BackTab => app.form_nav(-1),
            KeyCode::Left => app.form_cycle(-1),
            KeyCode::Right => app.form_cycle(1),
            KeyCode::Backspace => app.form_backspace(),
            KeyCode::Enter => app.submit_form(),
            _ if accepts_text_input(key).is_some() => {
                app.form_type(accepts_text_input(key).unwrap())
            }
            _ => {}
        }
        return;
    }

    if app.palette_open {
        app.clear_leader_sequence();
        if !matches!(key.code, KeyCode::Char('g')) {
            app.pending_g = false;
        }

        match key.code {
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE && app.palette_vim_nav => {
                app.open_palette()
            }
            KeyCode::Esc => {
                if app.prompt_session.is_some()
                    || app.palette_vim_nav
                    || app.palette_query.is_empty()
                {
                    app.close_command_prompt()
                } else {
                    app.enter_palette_vim_nav()
                }
            }
            KeyCode::Enter => app.run_command(),
            KeyCode::Up if key.modifiers == KeyModifiers::NONE && app.prompt_session.is_none() => {
                app.history_prev()
            }
            KeyCode::Down
                if key.modifiers == KeyModifiers::NONE && app.prompt_session.is_none() =>
            {
                app.history_next()
            }
            KeyCode::Tab if key.modifiers == KeyModifiers::NONE => app.apply_selected_suggestion(),
            KeyCode::Backspace => app.palette_backspace(),
            KeyCode::Char('j') if key.modifiers == KeyModifiers::NONE && app.palette_vim_nav => {
                app.palette_down()
            }
            KeyCode::Char('k') if key.modifiers == KeyModifiers::NONE && app.palette_vim_nav => {
                app.palette_up()
            }
            _ if accepts_text_input(key).is_some() => {
                app.palette_type(accepts_text_input(key).unwrap())
            }
            _ => {}
        }
        return;
    }

    if app.mode == super::app::Mode::Insert {
        app.clear_leader_sequence();
        match key.code {
            KeyCode::Esc => app.exit_insert(),
            KeyCode::Enter => app.notes_newline(),
            KeyCode::Backspace => app.notes_backspace(),
            _ if accepts_text_input(key).is_some() => {
                app.notes_type(accepts_text_input(key).unwrap())
            }
            // Ctrl-S: save the notes draft to the DB (Phase 7), with the
            // default commit message filled in for the current task.
            KeyCode::Char('s') if key.modifiers == KeyModifiers::CONTROL => {
                let key = app.current_task().map(|t| t.task_key.clone());
                let msg = match key {
                    Some(k) => format!("save notes for {k}"),
                    None => "save notes".to_string(),
                };
                app.save_notes_draft(Some(&msg));
            }
            _ => {}
        }
        return;
    }

    if key.modifiers != KeyModifiers::NONE {
        app.clear_leader_sequence();
    } else {
        if matches!(key.code, KeyCode::Char(' ')) {
            app.pending_space = true;
            app.pending_space_f = false;
            app.pending_g = false;
            return;
        }
        if app.pending_space {
            if matches!(key.code, KeyCode::Char('f')) {
                app.pending_space = false;
                app.pending_space_f = true;
                app.pending_g = false;
                return;
            }
            app.clear_leader_sequence();
        } else if app.pending_space_f {
            if matches!(key.code, KeyCode::Char('f')) {
                app.clear_leader_sequence();
                app.pending_g = false;
                app.open_task_project_finder();
                return;
            }
            if matches!(key.code, KeyCode::Char('g')) {
                app.clear_leader_sequence();
                app.pending_g = false;
                app.open_log_finder();
                return;
            }
            app.clear_leader_sequence();
        }
    }

    match key.code {
        KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => app.open_palette(),
        KeyCode::Char('?') if accepts_shifted_shortcut(key) => app.enter_view(View::Help, ""),
        KeyCode::Char(':') if accepts_shifted_shortcut(key) => app.open_palette(),
        KeyCode::Esc | KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => app.go_back(),
        KeyCode::Char('q') if key.modifiers == KeyModifiers::NONE => app.quit = true,
        KeyCode::Char('t') if key.modifiers == KeyModifiers::NONE => app.open_current_task(),
        KeyCode::Char('o') if key.modifiers == KeyModifiers::NONE && app.view == View::Task => {
            app.open_current_item()
        }
        KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
            app.toggle_complete_current_task()
        }
        KeyCode::Char('e') if key.modifiers == KeyModifiers::NONE && app.view == View::Task => {
            app.enter_notes_edit()
        }
        // `i` (vim insert): same as `e` in the task view — notes editing.
        KeyCode::Char('i') if key.modifiers == KeyModifiers::NONE && app.view == View::Task => {
            app.enter_notes_edit()
        }
        // `h`/`l` are reserved horizontal vim motions: lun's views are
        // vertical, so they are no-ops (the plan keeps the keymap vim-shaped).
        KeyCode::Char('h') | KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            if app.view == View::Task {
                app.task_focus_prev()
            }
        }
        KeyCode::Char('l') | KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            if app.view == View::Task {
                app.task_focus_next()
            }
        }
        KeyCode::Char('j') | KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
            match app.view {
                View::Project => app.project_nav(1, false),
                View::Task => {
                    if !app.task_item_nav(1) {
                        app.task_nav(1);
                    }
                }
                _ => app.task_nav(1),
            }
        }
        KeyCode::Char('k') | KeyCode::Up if key.modifiers == KeyModifiers::NONE => match app.view {
            View::Project => app.project_nav(-1, false),
            View::Task => {
                if !app.task_item_nav(-1) {
                    app.task_nav(-1);
                }
            }
            _ => app.task_nav(-1),
        },
        KeyCode::PageDown if key.modifiers == KeyModifiers::NONE => app.page_nav(1),
        KeyCode::PageUp if key.modifiers == KeyModifiers::NONE => app.page_nav(-1),
        KeyCode::Home if key.modifiers == KeyModifiers::NONE => app.jump_top(),
        KeyCode::End if key.modifiers == KeyModifiers::NONE => app.jump_bottom(),
        KeyCode::Char('g') if key.modifiers == KeyModifiers::NONE => {
            if app.pending_g {
                app.jump_top();
                app.pending_g = false;
            } else {
                app.pending_g = true;
            }
        }
        KeyCode::Char('G')
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            app.jump_bottom()
        }
        KeyCode::Enter if app.view == View::Project => app.project_nav(0, true),
        KeyCode::Enter if matches!(app.view, View::Status | View::Board) => app.open_current_task(),
        KeyCode::Enter if app.view == View::Task => app.open_current_item(),
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
