//! Phase 5 tests: TUI skeleton (purple theme, no editing).
//!
//! Two layers are exercised:
//! - `term::handle_key` — the pure key dispatcher (palette open/close,
//!   filtering, navigation, command execution, quit, project navigation).
//! - `render::paint` — headless rendering into a `ratatui::buffer::Buffer`
//!   (`Buffer::empty(Rect)`), asserting the spec's visible guarantees:
//!   banner + context block + board preview on the initial screen, the
//!   slash palette with its filtered rows and inverted selection, and the
//!   /status, /board, /project views.
//!
//! Plus `tui::data::load` against a real DB fixture: board column
//! partitioning and the summary line's exact per-status counts.

use lun::tui::app::{App, View};
use lun::tui::data;
use lun::tui::render;
use lun::tui::term;
use lun::{Lun, ProjectSpec, TaskSpec};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Render one frame of `app` at w×h and return the screen as lines of text.
fn screen(app: &App, w: u16, h: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    render::paint(&mut buf, Rect::new(0, 0, w, h), app);
    let content = buf.content();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(content[y as usize * w as usize + x as usize].symbol());
        }
        out.push('\n');
    }
    out
}

/// Render a frame and return a specific cell's symbol+style.
fn cell(app: &App, w: u16, h: u16, x: u16, y: u16) -> (char, Color, Color, bool) {
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    render::paint(&mut buf, Rect::new(0, 0, w, h), app);
    let c = buf.get(x, y);
    (
        c.symbol().chars().next().unwrap_or(' '),
        c.fg,
        c.bg,
        c.modifier.contains(Modifier::BOLD),
    )
}

fn line_with(haystack: &[&str], needle: &str) -> Option<usize> {
    haystack.iter().position(|line| line.contains(needle))
}

fn temp_root(name: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lun-p5-test-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Fixture: project "paper-stack" with one task in each status.
fn fixture() -> (std::path::PathBuf, Lun) {
    let root = temp_root("fx");
    let lun = Lun::init(&root).unwrap();
    let p1 = lun
        .create_project(ProjectSpec {
            name: "paper-stack".into(),
            ..Default::default()
        })
        .unwrap();
    for (title, status, priority) in [
        ("set up sim", "todo", "med"),
        ("tune damping", "in-progress", "high"),
        ("write methods", "review", "low"),
        ("implement restitution", "done", "med"),
    ] {
        lun.create_task(TaskSpec {
            title: title.into(),
            project: Some(p1.id),
            status: Some(status.into()),
            priority: Some(priority.into()),
            assignee: Some("me".into()),
            branch: None,
            labels: None,
            message: None,
            user: None,
        })
        .unwrap();
    }
    (root, lun)
}

fn app_for(root: &std::path::Path) -> App {
    let lun = Lun::open(root).unwrap();
    let d = data::load(&lun, "0.1.0", "repo-path", "main".into(), None).unwrap();
    App::new(d)
}

// ---------------------------------------------------------------------------
// Data layer
// ---------------------------------------------------------------------------

#[test]
fn data_load_seeds_unassigned_and_current_project() {
    let (_root, lun) = fixture();
    let d = data::load(&lun, "0.1.0", "repo-path", "main".into(), None).unwrap();
    // P-000 Unassigned seed + paper-stack
    assert_eq!(d.projects.len(), 2);
    assert_eq!(d.projects[0].project_key, "P-000");
    // current project defaults to first project
    assert_eq!(d.current_project, 0);
    // loading with an explicit key keeps that project current
    let d2 = data::load(&lun, "0.1.0", "repo-path", "main".into(), Some("P-001")).unwrap();
    assert_eq!(d2.projects[d2.current_project].name, "paper-stack");
}

#[test]
fn data_board_columns_partition_by_status() {
    let (root, lun) = fixture();
    let d = data::load(&lun, "0.1.0", "p", "b".into(), Some("P-001")).unwrap();
    let cols = d.board_columns(2); // paper-stack is row id 2 (P-000=1)
    let titles: Vec<Vec<&str>> = cols
        .iter()
        .map(|c| c.iter().map(|t| t.title.as_str()).collect())
        .collect();
    assert_eq!(titles[0], vec!["set up sim"]);
    assert_eq!(titles[1], vec!["tune damping"]);
    assert_eq!(titles[2], vec!["write methods"]);
    assert_eq!(titles[3], vec!["implement restitution"]);
    drop(lun);
    drop(root);
}

#[test]
fn data_summary_counts_exact() {
    let (root, lun) = fixture();
    let d = data::load(&lun, "0.1.0", "p", "b".into(), None).unwrap();
    assert_eq!(
        d.summary(),
        "2 projects \u{b7} 4 tasks (1 todo, 1 in-progress, 1 review, 1 done)"
    );
    drop(lun);
    drop(root);
}

// ---------------------------------------------------------------------------
// Palette state machine
// ---------------------------------------------------------------------------

#[test]
fn palette_opens_filters_and_executes() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);

    // Fresh app: palette closed, no quit.
    assert!(!app.palette_open);
    assert!(!app.quit);

    term::handle_key(&mut app, &key(KeyCode::Char('/')));
    assert!(app.palette_open);
    assert_eq!(app.palette_query, "");
    assert_eq!(app.filtered_commands().len(), 11);

    // Filtering narrows the list (the plan's "/sta" example shape).
    term::handle_key(&mut app, &key(KeyCode::Char('s')));
    term::handle_key(&mut app, &key(KeyCode::Char('t')));
    term::handle_key(&mut app, &key(KeyCode::Char('a')));
    assert_eq!(
        app.filtered_commands()
            .iter()
            .map(|c| c.name)
            .collect::<Vec<_>>(),
        vec!["/status"]
    );

    // Backspace restores the wider match ("st" still matches only /status).
    term::handle_key(&mut app, &key(KeyCode::Backspace));
    assert_eq!(app.filtered_commands().len(), 1); // /status

    // Down then up wraps within the filtered list.
    app.palette_down();
    assert_eq!(app.palette_selected, 0); // only one match: wraps to itself
    app.palette_up();
    assert_eq!(app.palette_selected, 0);

    // Enter runs the selected command: /status -> View::Status, palette closed.
    term::handle_key(&mut app, &key(KeyCode::Enter));
    assert!(!app.palette_open);
    assert_eq!(app.view, View::Status);

    // Re-open and run /quit.
    term::handle_key(&mut app, &key(KeyCode::Char('/')));
    app.palette_query.clear();
    for (i, c) in app.filtered_commands().iter().enumerate() {
        if c.name == "/quit" {
            app.palette_selected = i;
        }
    }
    term::handle_key(&mut app, &key(KeyCode::Enter));
    assert!(app.quit);
}

#[test]
fn palette_esc_closes_without_running() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char('/')));
    term::handle_key(&mut app, &key(KeyCode::Char('q'))); // filters to /quit
    assert!(!app.quit); // typing must not execute
    term::handle_key(&mut app, &key(KeyCode::Esc));
    assert!(!app.palette_open);
    assert!(!app.quit);
    assert_eq!(app.view, View::Initial);
}

#[test]
fn palette_no_match_shows_error_message() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char('/')));
    for c in "xyz".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    assert!(app.filtered_commands().is_empty());
    term::handle_key(&mut app, &key(KeyCode::Enter));
    let (msg, is_err) = app.message.clone().unwrap();
    assert!(is_err);
    assert!(msg.contains("xyz"));
    assert!(!app.palette_open);
}

// ---------------------------------------------------------------------------
// Outside-palette keys
// ---------------------------------------------------------------------------

#[test]
fn q_quits_outside_palette() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    assert!(!app.quit);
    term::handle_key(&mut app, &key(KeyCode::Char('q')));
    assert!(app.quit);
}

#[test]
fn project_view_j_k_enter_navigation() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Project;
    // fixture has P-000 (idx 0) and paper-stack (idx 1)
    assert_eq!(app.project_selected, 0);
    term::handle_key(&mut app, &key(KeyCode::Char('j')));
    assert_eq!(app.project_selected, 1);
    term::handle_key(&mut app, &key(KeyCode::Char('j'))); // wraps to 0
    assert_eq!(app.project_selected, 0);
    term::handle_key(&mut app, &key(KeyCode::Char('k'))); // wraps to last
    assert_eq!(app.project_selected, 1);
    term::handle_key(&mut app, &key(KeyCode::Enter));
    assert_eq!(app.data.current_project, 1);
    let (msg, is_err) = app.message.clone().unwrap();
    assert!(!is_err);
    assert!(msg.contains("paper-stack") && msg.contains("P-001"));

    // j/k in other views do nothing (Phase 5 scope).
    app.view = View::Status;
    let before = app.project_selected;
    term::handle_key(&mut app, &key(KeyCode::Char('j')));
    assert_eq!(app.project_selected, before);
}

// ---------------------------------------------------------------------------
// Headless rendering
// ---------------------------------------------------------------------------

#[test]
fn initial_screen_renders_banner_context_and_board() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    // Make paper-stack current so the board preview has tasks.
    app.data.current_project = 1;
    let s = screen(&app, 80, 24);

    // Banner: the spec's ASCII art, bright purple + bold. Line 0 is
    // "    _                    _" so the first non-space glyph is '_' at x=4;
    // line 1 carries the pipe strokes.
    assert!(s.contains("| | ___  _ __"));
    let (ch, fg, _bg, bold) = cell(&app, 80, 24, 3, 1);
    assert_eq!(ch, '|');
    assert_eq!(fg, Color::Rgb(177, 121, 255));
    assert!(bold);

    // Subtitle + magenta separator.
    assert!(s.contains("lun v0.1.0 \u{2014} CLI-first markdown task & project tracker"));

    // Context block.
    assert!(s.contains("Repo:"));
    assert!(s.contains("Branch:"));
    assert!(s.contains("main"));
    assert!(s.contains("Project:"));
    assert!(s.contains("paper-stack"));
    assert!(s.contains("2 projects \u{b7} 4 tasks (1 todo, 1 in-progress, 1 review, 1 done)"));

    // Board preview sections + task keys/titles.
    assert!(s.contains("BOARD (PAPER-STACK)"));
    assert!(s.contains("T-001"));
    assert!(s.contains("set up sim"));
    assert!(s.contains("tune damping"));

    // Hint bar on the last row with the purple prompt symbol.
    let last = s.lines().last().unwrap();
    assert!(last.contains("type \"/\" for commands, \":\" for quick actions, \"q\" to quit"));
    let (prompt, pfg, ..) = cell(&app, 80, 24, 0, 23);
    assert_eq!(prompt, '\u{203a}');
    assert_eq!(pfg, Color::Rgb(177, 121, 255));

    // Dark navy background everywhere.
    let (_c, _fg, bg, _bold) = cell(&app, 80, 24, 79, 0);
    assert_eq!(bg, Color::Rgb(13, 17, 28));
}

#[test]
fn palette_renders_filtered_rows_with_inverted_selection() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char('/')));
    for c in "st".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    assert_eq!(app.filtered_commands().len(), 1);
    let s = screen(&app, 80, 24);
    assert!(s.contains("COMMANDS"));
    assert!(s.contains("/status"));
    assert!(s.contains("Show global status"));
    // /board must be filtered out by "st".
    assert!(!s.contains("/board"));

    // Query renders on the bottom prompt line.
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines[23].contains("› /st_"));

    // Selected row is inverted: purple background and now bottom-anchored.
    let sep_y = 24u16 - 2;
    let selected_y = sep_y - 1;
    let (ch, fg, bg, _bold) = cell(&app, 80, 24, 0, selected_y);
    assert_eq!(ch, '/');
    assert_eq!(bg, Color::Rgb(177, 121, 255));
    assert_eq!(fg, Color::Rgb(13, 17, 28));
}

#[test]
fn palette_query_renders_on_bottom_prompt_line() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.palette_open = true;
    app.palette_query = "this is me typing".to_string();
    let s = screen(&app, 80, 24);
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines[23].contains("/this is me typing"));
    assert!(!lines[0].contains("/this is me typing"));
}

#[test]
fn palette_commands_render_above_separator() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.palette_open = true;
    app.palette_query = "st".to_string();
    let s = screen(&app, 80, 24);
    let lines: Vec<&str> = s.lines().collect();
    let sep_y = 24usize - 2;

    let heading_y = line_with(&lines, "COMMANDS").unwrap();
    let row_y = line_with(&lines, "/status").unwrap();

    assert!(heading_y < sep_y);
    assert_eq!(row_y, sep_y - 1);
    assert!(row_y < sep_y);
}

#[test]
fn palette_does_not_clobber_content_top() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Initial;
    app.palette_open = true;
    let s = screen(&app, 80, 24);
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines[0].contains("    _                    _"));
    assert!(lines[1].contains("| |    _   _ _ __"));
}

#[test]
fn palette_selection_highlight_still_applies() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.palette_open = true;
    app.palette_query = "st".to_string();
    let sep_y = 24u16 - 2;
    let selected_y = sep_y - 1;
    let (ch, fg, bg, _bold) = cell(&app, 80, 24, 0, selected_y);
    assert_eq!(ch, '/');
    assert_eq!(bg, Color::Rgb(177, 121, 255));
    assert_eq!(fg, Color::Rgb(13, 17, 28));
}

#[test]
fn long_command_list_is_clamped_to_available_rows() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.palette_open = true;
    app.palette_query.clear();
    // 80x10 gives only 8 rows above the separator for the palette list.
    let s = screen(&app, 80, 10);
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines.len(), 10);
    assert!(lines[8].chars().all(|c| c == '-'));
    assert!(lines[9].contains('›'));
}

#[test]
fn statusline_query_renders_on_bottom_prompt_line() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.statusline_open = true;
    app.statusline_query = "project".to_string();
    let s = screen(&app, 80, 24);
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines[23].contains("status project"));
    assert!(!lines[0].contains("status project"));
}

#[test]
fn status_view_lists_projects_and_all_tasks() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Status;
    let s = screen(&app, 80, 30);
    assert!(s.contains("STATUS"));
    assert!(s.contains("PROJECTS"));
    assert!(s.contains("P-000"));
    assert!(s.contains("paper-stack"));
    assert!(s.contains("ALL TASKS"));
    assert!(s.contains("T-001"));
    assert!(s.contains("T-004"));
    assert!(s.contains("2 projects \u{b7} 4 tasks"));
}

#[test]
fn board_view_renders_four_columns() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.data.current_project = 1;
    app.view = View::Board;
    let s = screen(&app, 120, 24);
    assert!(s.contains("BOARD (PAPER-STACK)"));
    for col in ["Todo", "In Progress", "Review", "Done"] {
        assert!(s.contains(col), "missing column {col}");
    }
    assert!(s.contains("T-001"));
    assert!(s.contains("T-002"));
    assert!(s.contains("T-003"));
    assert!(s.contains("T-004"));
}

#[test]
fn project_view_marks_selected_and_current() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Project;
    app.data.current_project = 1;
    let s = screen(&app, 80, 24);
    assert!(s.contains("PROJECT"));
    assert!(s.contains("P-000"));
    assert!(s.contains("P-001"));
    // Selection marker ">" on the currently selected row (idx 0).
    assert!(s.lines().any(|l| l.starts_with("> P-000")));
    // Current-project marker "*" on idx 1.
    assert!(s.lines().any(|l| l.starts_with("* P-001")));
}

#[test]
fn help_view_lists_keybindings() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Help;
    let s = screen(&app, 80, 24);
    assert!(s.contains("HELP"));
    assert!(s.contains("open the command palette"));
    assert!(s.contains("quit lun"));
}

#[test]
fn placeholder_view_says_coming_soon() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Placeholder;
    let s = screen(&app, 80, 24);
    assert!(s.contains("COMING SOON"));
    assert!(s.contains("planned for a later phase"));
}

#[test]
fn message_line_overlays_above_hint_bar() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.message = Some(("boom".to_string(), true));
    let s = screen(&app, 80, 24);
    let lines: Vec<&str> = s.lines().collect();
    // Bottom bar: prompt on last line (23), separator (22), message (21).
    assert!(lines[21].contains("boom"));
    assert!(lines[22].chars().all(|c| c == '-'));
    assert!(lines[23].contains("q\" to quit"));
    let (_c, fg, _bg, bold) = cell(&app, 80, 24, 0, 21);
    assert_eq!(fg, Color::Rgb(255, 80, 80));
    assert!(bold);
}

// Rendering must never panic on tiny screens (saturating clamps everywhere).
#[test]
fn tiny_screen_does_not_panic() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    for view in [
        View::Initial,
        View::Status,
        View::Board,
        View::Project,
        View::Help,
        View::Placeholder,
    ] {
        app.view = view;
        let _ = screen(&app, 5, 3);
    }
    app.palette_open = true;
    let _ = screen(&app, 5, 3);
}
