//! Phase 6 tests: TUI task view, vim keybindings, and logs.
//!
//! Same two-layer approach as tests/phase5.rs:
//! - `term::handle_key` — the pure key dispatcher (normal/insert modes,
//!   statusline, task navigation, palette command lines).
//! - `render::paint` — headless rendering into a `ratatui::buffer::Buffer`,
//!   asserting the task view, log view, and mode-aware hint bar.
//! - `tui::data` — snapshot loading (logs, attachments, links) and the
//!   `:status` / `/log` query resolver against a real DB fixture.

use lun::tui::app::{App, Mode, TaskFocus, View};
use lun::tui::data;
use lun::tui::render;
use lun::tui::term;
use lun::{Lun, ProjectSpec, TaskSpec};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
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

fn temp_root(name: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lun-p6-test-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Fixture: project "paper-stack" (P-001) with four tasks, one attachment
/// and one link on T-001, plus a COMMENT log entry on T-001 so the log
/// view has something to show per task.
fn fixture() -> (std::path::PathBuf, Lun) {
    let root = temp_root("fx6");
    let lun = Lun::init(&root).unwrap();
    let p1 = lun
        .create_project(ProjectSpec {
            name: "paper-stack".into(),
            ..Default::default()
        })
        .unwrap();
    for (title, status, priority) in [
        ("set up sim", "todo", "med"),
        ("tune damping", "doing", "high"),
        ("write methods", "follow-up", "low"),
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
    // T-001 = "set up sim" (first real task, row id 1).
    lun.add_attachment(1, "mock.png", "/tmp/mock.png", None, None)
        .unwrap();
    lun.add_link(
        lun::LinkTarget::Task(1),
        "obsidian",
        "obsidian://open?vault=personal&file=ball-bounce",
        None,
        None,
    )
    .unwrap();
    lun.log(
        "task",
        1,
        "COMMENT",
        "",
        "{\"note\": \"watch damping\"}",
        None,
    )
    .unwrap();
    (root, lun)
}

fn app_for(root: &std::path::Path) -> App {
    let lun = Lun::open(root).unwrap();
    let d = data::load(&lun, "0.1.0", "repo-path", "main".into(), Some("P-001")).unwrap();
    App::with_store(d, root.to_path_buf(), lun)
}

fn app_with_store(root: &std::path::Path) -> App {
    app_for(root)
}

// ---------------------------------------------------------------------------
// Data layer (Phase 6 additions)
// ---------------------------------------------------------------------------

#[test]
fn data_load_snapshots_logs_attachments_links() {
    let (root, lun) = fixture();
    let d = data::load(&lun, "0.1.0", "p", "b".into(), None).unwrap();
    // T-001..T-004 each have a CREATE log; T-001 adds ATTACH + LINK + COMMENT.
    assert_eq!(d.logs.len(), 4 + 3);
    assert_eq!(d.attachments.len(), 1);
    assert_eq!(d.links.len(), 1);
    assert_eq!(d.logs_for_task(1).len(), 4); // CREATE, ATTACH, LINK, COMMENT
    assert_eq!(d.attachments_for_task(1).len(), 1);
    assert_eq!(d.links_for_task(1).len(), 1);
    assert_eq!(d.attachments_for_task(2).len(), 0);
    // Task logs come back newest first per task (DB order).
    let newest = d.logs_for_task(1)[0];
    assert_eq!(newest.action, "COMMENT");
    drop(lun);
    drop(root);
}

#[test]
fn data_project_log_merges_task_entries_newest_first() {
    let (root, lun) = fixture();
    let d = data::load(&lun, "0.1.0", "p", "b".into(), None).unwrap();
    let p_id = d.projects[1].id; // paper-stack
    let entries = d.project_log_entries(p_id);
    // CREATE project + 4 task CREATEs + ATTACH + LINK + COMMENT = 8.
    assert_eq!(entries.len(), 8);
    // Newest id first.
    for w in entries.windows(2) {
        assert!(w[0].id > w[1].id);
    }
    drop(lun);
    drop(root);
}

#[test]
fn resolve_log_query_precedence_and_ambiguity() {
    let (root, lun) = fixture();
    let d = data::load(&lun, "0.1.0", "p", "b".into(), None).unwrap();

    // Task key.
    match data::resolve_log_query(&d, "T-002").unwrap() {
        data::LogSubject::Task(i) => assert_eq!(d.tasks[i].task_key, "T-002"),
        other => panic!("expected task subject, got {other:?}"),
    }
    // Project key.
    match data::resolve_log_query(&d, "P-001").unwrap() {
        data::LogSubject::Project(i) => assert_eq!(d.projects[i].name, "paper-stack"),
        other => panic!("expected project subject, got {other:?}"),
    }
    // Exact task title.
    match data::resolve_log_query(&d, "tune damping").unwrap() {
        data::LogSubject::Task(i) => assert_eq!(d.tasks[i].task_key, "T-002"),
        other => panic!("expected task subject, got {other:?}"),
    }
    // Project name.
    assert!(matches!(
        data::resolve_log_query(&d, "paper-stack").unwrap(),
        data::LogSubject::Project(1)
    ));
    // No match / ambiguous / empty.
    assert!(data::resolve_log_query(&d, "nope").is_err());
    assert!(data::resolve_log_query(&d, "  ").is_err());
    drop(lun);
    drop(root);
}

#[test]
fn resolve_log_query_ambiguous_title_lists_keys() {
    let root = temp_root("amb");
    let lun = Lun::init(&root).unwrap();
    let p1 = lun
        .create_project(ProjectSpec {
            name: "proj".into(),
            ..Default::default()
        })
        .unwrap();
    for title in ["dup", "dup"] {
        lun.create_task(TaskSpec {
            title: title.into(),
            project: Some(p1.id),
            ..Default::default()
        })
        .unwrap();
    }
    let d = data::load(&lun, "0.1.0", "p", "b".into(), None).unwrap();
    let err = data::resolve_log_query(&d, "dup").unwrap_err();
    assert!(err.contains("ambiguous"), "got: {err}");
    assert!(err.contains("T-001") && err.contains("T-002"));
    drop(lun);
    drop(root);
}

// ---------------------------------------------------------------------------
// Key dispatch: normal mode + vim navigation
// ---------------------------------------------------------------------------

#[test]
fn t_opens_current_task_and_j_k_moves_task_selection() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    assert_eq!(app.task_selected, 0);
    assert!(app.current_task().unwrap().task_key == "T-001");

    // j/k move the task selection in non-project views.
    term::handle_key(&mut app, &key(KeyCode::Char('j')));
    assert_eq!(app.task_selected, 1);
    assert_eq!(app.current_task().unwrap().task_key, "T-002");
    term::handle_key(&mut app, &key(KeyCode::Char('k')));
    assert_eq!(app.task_selected, 0);
    // Wraps around.
    term::handle_key(&mut app, &key(KeyCode::Char('k')));
    assert_eq!(app.task_selected, 3);

    // t jumps to the current task's detail view (normal mode preserved).
    app.task_selected = 1;
    term::handle_key(&mut app, &key(KeyCode::Char('t')));
    assert_eq!(app.view, View::Task);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn t_does_nothing_when_no_tasks() {
    let root = temp_root("empty");
    let _lun = Lun::init(&root).unwrap();
    let mut app = app_for(&root);
    assert!(app.current_task().is_none());
    term::handle_key(&mut app, &key(KeyCode::Char('t')));
    assert_ne!(app.view, View::Task);
    let (msg, is_err) = app.message.clone().unwrap();
    assert!(is_err);
    assert!(msg.contains("no tasks"));
}

// ---------------------------------------------------------------------------
// Key dispatch: insert mode
// ---------------------------------------------------------------------------

#[test]
fn insert_mode_types_newlines_backspace_and_esc_returns() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char('t'))); // task view
    term::handle_key(&mut app, &key(KeyCode::Char('i')));
    assert_eq!(app.mode, Mode::Insert);

    for c in "hello".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    assert_eq!(app.notes_draft, "hello");
    assert!(app.notes_dirty);
    assert!(app.notes_modified());

    term::handle_key(&mut app, &key(KeyCode::Enter));
    assert_eq!(app.notes_draft, "hello\n");

    term::handle_key(&mut app, &key(KeyCode::Backspace));
    assert_eq!(app.notes_draft, "hello");

    // Esc returns to normal mode; the draft is kept as an unsaved draft.
    term::handle_key(&mut app, &key(KeyCode::Esc));
    assert_eq!(app.mode, Mode::Normal);
    assert!(!app.notes_modified());
    assert_eq!(app.notes_draft, "hello");
}

#[test]
fn enter_notes_edit_only_in_task_view() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Status;
    term::handle_key(&mut app, &key(KeyCode::Char('i')));
    assert_eq!(app.mode, Mode::Normal); // not in task view: no-op
    app.view = View::Task;
    term::handle_key(&mut app, &key(KeyCode::Char('i')));
    assert_eq!(app.mode, Mode::Insert);
}

#[test]
fn unsaved_note_blocks_view_switches_and_quit() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char('t')));
    term::handle_key(&mut app, &key(KeyCode::Char('i')));
    term::handle_key(&mut app, &key(KeyCode::Char('x')));
    assert!(app.notes_modified());

    // Switching views is blocked.
    term::handle_key(&mut app, &key(KeyCode::Char('/')));
    assert!(!app.palette_open);

    // q is blocked by the palette-level guard: run /quit with the palette.
    app.open_palette();
    app.palette_query = "quit".into();
    term::handle_key(&mut app, &key(KeyCode::Enter));
    assert!(!app.quit);
    let (msg, is_err) = app.message.clone().unwrap();
    assert!(is_err);
    assert!(msg.contains("unsaved"));

    // Esc out of insert (keeping the draft) unblocks everything.
    app.mode = Mode::Insert;
    term::handle_key(&mut app, &key(KeyCode::Esc));
    app.open_palette();
    app.palette_query = "quit".into();
    term::handle_key(&mut app, &key(KeyCode::Enter));
    assert!(app.quit);
}

#[test]
fn h_l_are_reserved_vim_noops() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char('j')));
    let sel = app.task_selected;
    term::handle_key(&mut app, &key(KeyCode::Char('h')));
    term::handle_key(&mut app, &key(KeyCode::Char('l')));
    assert_eq!(app.task_selected, sel);
    assert_eq!(app.view, View::Initial);
}

// ---------------------------------------------------------------------------
// Key dispatch: command prompt
// ---------------------------------------------------------------------------

#[test]
fn colon_opens_command_prompt_and_executes_status_query() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char(':')));
    assert!(app.palette_open);
    for c in "status T-001".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    term::handle_key(&mut app, &key(KeyCode::Enter));
    assert!(!app.palette_open);
    assert_eq!(app.view, View::Output);
    let out = app.output.as_ref().unwrap();
    assert!(out.text.contains("**Task T-001**"), "{}", out.text);
}

#[test]
fn bare_log_command_uses_current_project_context() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char('/')));
    for c in "log".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    term::handle_key(&mut app, &key(KeyCode::Enter));
    let out = app.output.as_ref().unwrap();
    assert!(out.text.contains("paper-stack"));
    assert!(out.text.contains("CREATE"));
}

#[test]
fn command_prompt_bad_target_shows_error() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char(':')));
    for c in "bogus foo".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    term::handle_key(&mut app, &key(KeyCode::Enter));
    let out = app.output.as_ref().unwrap();
    assert!(out.is_error);
    assert!(out.text.contains("bogus"));
    assert_eq!(app.view, View::Output);

    // Bad target: stays put, error shown.
    term::handle_key(&mut app, &key(KeyCode::Char(':')));
    for c in "status no-such-task".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    term::handle_key(&mut app, &key(KeyCode::Enter));
    let out = app.output.as_ref().unwrap();
    assert!(out.is_error);
    assert!(out.text.contains("no-such-task"));
}

#[test]
fn command_prompt_esc_closes_without_running() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char(':')));
    for c in "status T-001".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    term::handle_key(&mut app, &key(KeyCode::Esc));
    assert!(app.palette_open);
    assert!(app.palette_vim_nav);
    term::handle_key(&mut app, &key(KeyCode::Esc));
    assert!(!app.palette_open);
    assert_eq!(app.palette_query, "");
    assert_eq!(app.view, View::Initial);
}

#[test]
fn leader_space_f_f_opens_status_finder() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char(' ')));
    term::handle_key(&mut app, &key(KeyCode::Char('f')));
    term::handle_key(&mut app, &key(KeyCode::Char('f')));
    assert!(app.palette_open);
    assert_eq!(app.palette_query, "status ");
    assert!(!app.palette_vim_nav);
}

#[test]
fn leader_space_f_g_opens_text_grep_finder() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char(' ')));
    term::handle_key(&mut app, &key(KeyCode::Char('f')));
    term::handle_key(&mut app, &key(KeyCode::Char('g')));
    assert!(app.palette_open);
    assert_eq!(app.palette_query, "grep ");
    assert!(!app.palette_vim_nav);
}

#[test]
fn slash_prefixed_query_runs_status_in_tui_palette() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    open_palette_and_run(&mut app, "/paper-stack");
    let out = app.output.as_ref().unwrap();
    assert!(!out.is_error, "{}", out.text);
    assert!(out.text.contains("Project: paper-stack"), "{}", out.text);
}

#[test]
fn command_bracket_shortcuts_navigate_screen_history() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.enter_view(View::Project, "");
    app.enter_view(View::Help, "");
    assert_eq!(app.view, View::Help);

    term::handle_key(
        &mut app,
        &key_with_modifiers(KeyCode::Char('['), KeyModifiers::SUPER),
    );
    assert_eq!(app.view, View::Project);

    term::handle_key(
        &mut app,
        &key_with_modifiers(KeyCode::Char(']'), KeyModifiers::SUPER),
    );
    assert_eq!(app.view, View::Help);

    term::handle_key(&mut app, &key(KeyCode::Char('[')));
    assert_eq!(app.view, View::Project);

    term::handle_key(&mut app, &key(KeyCode::Char(']')));
    assert_eq!(app.view, View::Help);
}

#[test]
fn backspace_in_command_prompt_edits_query() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    term::handle_key(&mut app, &key(KeyCode::Char(':')));
    for c in "status T-002".chars() {
        term::handle_key(&mut app, &key(KeyCode::Char(c)));
    }
    for _ in 0..5 {
        term::handle_key(&mut app, &key(KeyCode::Backspace));
    }
    assert_eq!(app.palette_query, "status ");
}

// ---------------------------------------------------------------------------
// Palette command lines (/task, /log)
// ---------------------------------------------------------------------------

fn open_palette_and_run(app: &mut App, query: &str) {
    term::handle_key(app, &key(KeyCode::Char('/')));
    for c in query.chars() {
        term::handle_key(app, &key(KeyCode::Char(c)));
    }
    term::handle_key(app, &key(KeyCode::Enter));
}

#[test]
fn palette_task_command_line_selects_task() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    open_palette_and_run(&mut app, "task T-003");
    assert_eq!(app.view, View::Output);
    assert!(app.current_task().unwrap().task_key == "T-003");
    assert!(app.output.as_ref().unwrap().text.contains("Task T-003"));
}

#[test]
fn palette_task_by_title_selects_task() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    open_palette_and_run(&mut app, "task \"write methods\"");
    assert_eq!(app.view, View::Output);
    assert!(app.current_task().unwrap().task_key == "T-003");
}

#[test]
fn palette_task_bad_key_shows_error_and_stays() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    open_palette_and_run(&mut app, "task T-999");
    assert_eq!(app.view, View::Output);
    let out = app.output.as_ref().unwrap();
    assert!(out.is_error);
    assert!(out.text.contains("T-999"));
}

#[test]
fn palette_log_command_line_selects_subject() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    open_palette_and_run(&mut app, "log T-004");
    assert_eq!(app.view, View::Output);
    assert!(app.output.as_ref().unwrap().text.contains("CREATE"));
    assert!(app.current_task().unwrap().task_key == "T-004");
}

#[test]
fn palette_log_default_is_current_project() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root); // current project: paper-stack (idx 1)
    open_palette_and_run(&mut app, "log");
    assert_eq!(app.view, View::Output);
    assert!(app.output.as_ref().unwrap().text.contains("paper-stack"));
}

#[test]
fn palette_task_command_with_spaces_between_words() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    // "/task  T-001" (double space) still works: rest is trimmed.
    open_palette_and_run(&mut app, "task  T-001");
    assert_eq!(app.view, View::Output);
    assert!(app.current_task().unwrap().task_key == "T-001");
}

// ---------------------------------------------------------------------------
// Headless rendering
// ---------------------------------------------------------------------------

#[test]
fn task_view_renders_fields_notes_and_history() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Task; // T-001: has attachment, link, COMMENT
    let s = screen(&app, 90, 40);

    assert!(s.contains("Task T-001"));
    assert!(s.contains("Project:"));
    assert!(s.contains("paper-stack"));
    assert!(s.contains("Title:"));
    assert!(s.contains("set up sim"));
    assert!(s.contains("Status:"));
    assert!(s.contains("Priority:"));
    assert!(s.contains("Assignee:"));
    assert!(s.contains("Description:"));
    assert!(s.contains("Attachments:"));
    assert!(s.contains("mock.png"));
    assert!(s.contains("Links:"));
    assert!(s.contains("obsidian"));
    assert!(s.contains("Last Commit:"));
    assert!(!s.contains("History:"));
    // Task view shows only the latest CLI-format entry for the task.
    assert!(s.contains("me"));
    assert!(s.contains("COMMENT"));
    assert!(s.contains("Commit:"));
    assert!(s.contains("watch damping"));
}

#[test]
fn task_view_empty_state_has_no_task() {
    let root = temp_root("empty2");
    let _lun = Lun::init(&root).unwrap();
    let mut app = app_for(&root);
    app.view = View::Task;
    let s = screen(&app, 80, 24);
    assert!(s.contains("no task selected"));
}

#[test]
fn task_view_insert_mode_shows_draft_lines() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Task;
    app.mode = Mode::Insert;
    app.notes_draft = "first line\nsecond line".into();
    let s = screen(&app, 90, 40);
    // Each draft line renders as a "- " bullet under Description:.
    let notes_idx = s.find("Description:").expect("Description: section");
    let tail = &s[notes_idx..];
    assert!(tail.contains("- first line"));
    assert!(tail.contains("- second line"));
    // Hint bar switches to the insert-mode hint.
    let last = s.lines().last().unwrap();
    assert!(last.contains("editing description"));
}

#[test]
fn log_view_task_renders_header_and_entries() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.log_subject = Some(data::LogSubject::Task(0));
    app.view = View::Log;
    let s = screen(&app, 90, 40);
    assert!(s.contains("Log: Task T-001 \"set up sim\""));
    assert!(s.contains("CREATE"));
    assert!(s.contains("ATTACH"));
    assert!(s.contains("LINK"));
    assert!(s.contains("COMMENT"));
    assert!(s.contains("Commit:"));
    assert!(s.contains("watch damping"));
}

#[test]
fn log_view_project_renders_merged_newest_first() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.log_subject = Some(data::LogSubject::Project(1));
    app.view = View::Log;
    let s = screen(&app, 90, 50);
    assert!(s.contains("Log: paper-stack"));
    // Project logs render the commit MESSAGE per line (not the action
    // word), newest first: the last task created ("implement restitution",
    // highest id) must appear before the first ("set up sim").
    let newest = s
        .find("add task \"implement restitution\"")
        .expect("newest task create");
    let oldest = s
        .find("add task \"set up sim\"")
        .expect("oldest task create");
    assert!(newest < oldest, "newest-first order violated");
    // The project's own CREATE entry (its commit message) is present.
    assert!(s.contains("create project \"paper-stack\""));
}

#[test]
fn log_view_without_subject_says_why() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Log;
    let s = screen(&app, 80, 24);
    assert!(s.contains("no log subject"));
}

#[test]
fn statusline_renders_prompt_on_hint_bar() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.palette_open = true;
    app.palette_query = "status T-001".into();
    let s = screen(&app, 80, 24);
    let last = s.lines().last().unwrap();
    assert!(last.contains("status T-001"));
}

#[test]
fn help_view_lists_phase6_keys() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Help;
    let s = screen(&app, 90, 30);
    assert!(s.contains("HELP"));
    assert!(s.contains(":status"));
    assert!(s.contains("open the current task"));
    assert!(s.contains("quit lun"));
}

#[test]
fn arrows_home_end_page_and_gg_g_navigate() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Status;
    term::handle_key(&mut app, &key(KeyCode::Down));
    assert_eq!(app.current_task().unwrap().task_key, "T-002");
    term::handle_key(&mut app, &key(KeyCode::End));
    assert_eq!(app.current_task().unwrap().task_key, "T-004");
    term::handle_key(&mut app, &key(KeyCode::Home));
    assert_eq!(app.current_task().unwrap().task_key, "T-001");
    term::handle_key(&mut app, &key(KeyCode::PageDown));
    assert_eq!(app.current_task().unwrap().task_key, "T-002");
    term::handle_key(&mut app, &key(KeyCode::Char('G')));
    assert_eq!(app.current_task().unwrap().task_key, "T-004");
    term::handle_key(&mut app, &key(KeyCode::Char('g')));
    term::handle_key(&mut app, &key(KeyCode::Char('g')));
    assert_eq!(app.current_task().unwrap().task_key, "T-001");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn question_mark_help_backspace_and_task_open_focus_behave() {
    let (root, _lun) = fixture();
    let mut app = app_for(&root);
    app.view = View::Status;
    term::handle_key(&mut app, &key(KeyCode::Char('?')));
    assert_eq!(app.view, View::Help);
    term::handle_key(&mut app, &key(KeyCode::Backspace));
    assert_eq!(app.view, View::Status);

    app.view = View::Task;
    assert_eq!(app.task_focus, TaskFocus::Summary);
    term::handle_key(&mut app, &key(KeyCode::Right));
    assert_eq!(app.task_focus, TaskFocus::Notes);
    term::handle_key(&mut app, &key(KeyCode::Right));
    assert_eq!(app.task_focus, TaskFocus::Attachments);
    term::handle_key(&mut app, &key(KeyCode::Backspace));
    assert_eq!(app.task_focus, TaskFocus::Summary);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn o_opens_selected_attachment_and_c_toggles_completion_with_store() {
    let (root, _lun) = fixture();
    let mut app = app_with_store(&root);
    app.view = View::Task;
    term::handle_key(&mut app, &key(KeyCode::Right));
    term::handle_key(&mut app, &key(KeyCode::Right));
    std::env::set_var("LUN_OPEN_BIN", "true");
    term::handle_key(&mut app, &key(KeyCode::Enter));
    let (msg, is_err) = app.message.clone().unwrap();
    assert!(!is_err);
    assert!(msg.contains("Opened:"));
    term::handle_key(&mut app, &key(KeyCode::Char('o')));
    let (msg, is_err) = app.message.clone().unwrap();
    assert!(!is_err);
    assert!(msg.contains("Opened:"));

    term::handle_key(&mut app, &key(KeyCode::Char('c')));
    assert_eq!(app.current_task().unwrap().status, "done");
    term::handle_key(&mut app, &key(KeyCode::Char('c')));
    assert_eq!(app.current_task().unwrap().status, "doing");
    std::env::remove_var("LUN_OPEN_BIN");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn new_task_form_creates_tasks_in_selected_projects() {
    let (root, _lun) = fixture();
    let mut app = app_with_store(&root);

    app.enter_view(View::NewTask, "task on project");
    for _ in 0..7 {
        app.form_nav(1);
    }
    app.submit_form();
    let t1 = app.lun.as_ref().unwrap().task_by_key("T-005").unwrap();
    let p1 = app.lun.as_ref().unwrap().project_by_key("P-001").unwrap();
    assert_eq!(t1.project_id, Some(p1.id));

    app.enter_view(View::NewTask, "task on unassigned");
    app.form_nav(1); // Project field
    app.form_cycle(-1); // Project -> P-000
    for _ in 0..6 {
        app.form_nav(1);
    }
    app.submit_form();
    let t2 = app.lun.as_ref().unwrap().task_by_key("T-006").unwrap();
    let p0 = app.lun.as_ref().unwrap().project_by_key("P-000").unwrap();
    assert_eq!(t2.project_id, Some(p0.id));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn new_project_form_creates_project_and_selects_it() {
    let (root, _lun) = fixture();
    let mut app = app_with_store(&root);

    app.enter_view(View::NewProject, "");
    for ch in "infra".chars() {
        app.form_type(ch);
    }
    app.form_nav(1);
    app.form_cycle(1); // inactive
    app.form_nav(1);
    app.submit_form();

    let p = app.lun.as_ref().unwrap().project_by_name("infra").unwrap();
    assert_eq!(p.project_key, "P-002");
    assert_eq!(p.status, "inactive");
    assert_eq!(
        app.data.projects[app.data.current_project].project_key,
        "P-002"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn new_task_form_esc_enters_vim_nav_and_double_esc_cancels() {
    let (root, _lun) = fixture();
    let mut app = app_with_store(&root);
    let starting_tasks = app.data.tasks.len();

    app.enter_view(View::NewTask, "");
    term::handle_key(
        &mut app,
        &key_with_modifiers(KeyCode::Char('A'), KeyModifiers::SHIFT),
    );
    term::handle_key(
        &mut app,
        &key_with_modifiers(KeyCode::Char('_'), KeyModifiers::SHIFT),
    );
    if let Some(lun::tui::app::FormState::NewTask(draft)) = app.form() {
        assert_eq!(draft.title, "A_");
    } else {
        panic!("expected new-task form");
    }

    term::handle_key(&mut app, &key(KeyCode::Esc));
    assert!(app.form().is_some());
    assert!(app.form_vim_nav);
    if let Some(lun::tui::app::FormState::NewTask(draft)) = app.form() {
        assert_eq!(draft.field, 0);
    } else {
        panic!("expected new-task form");
    }

    term::handle_key(&mut app, &key(KeyCode::Char('j')));
    if let Some(lun::tui::app::FormState::NewTask(draft)) = app.form() {
        assert_eq!(draft.field, 1);
    } else {
        panic!("expected new-task form");
    }

    term::handle_key(&mut app, &key(KeyCode::Esc));
    assert!(app.form().is_some());
    term::handle_key(&mut app, &key(KeyCode::Esc));
    assert!(app.form().is_none());
    assert_eq!(app.data.tasks.len(), starting_tasks);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn move_form_reassigns_task_and_writes_update_log() {
    let (root, _lun) = fixture();
    let mut app = app_with_store(&root);
    app.view = View::Task;
    assert_eq!(app.current_task().unwrap().task_key, "T-001");

    app.enter_view(View::MoveTask, "");
    app.form_cycle(-1); // move to P-000 Unassigned
    app.form_nav(1);
    app.submit_form();

    let t = app.lun.as_ref().unwrap().task_by_key("T-001").unwrap();
    let p0 = app.lun.as_ref().unwrap().project_by_key("P-000").unwrap();
    assert_eq!(t.project_id, Some(p0.id));
    let logs = app.lun.as_ref().unwrap().logs_for("task", t.id).unwrap();
    assert!(logs
        .iter()
        .any(|e| e.message.contains("move T-001 to Unassigned")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dropped_paths_support_escaped_quoted_and_file_uri_without_copy() {
    let (root, _lun) = fixture();
    let mut app = app_with_store(&root);
    app.view = View::Task;

    let spaced = root.join("My File.pdf");
    let plain = root.join("second.txt");
    let local = root.join("third.txt");
    std::fs::write(&spaced, "a").unwrap();
    std::fs::write(&plain, "b").unwrap();
    std::fs::write(&local, "c").unwrap();

    let escaped_spaced = spaced.display().to_string().replace(' ', "\\ ");
    let drop_text = format!(
        "{} file://{} file://localhost{}",
        escaped_spaced,
        plain.display(),
        local.display()
    );
    app.attach_dropped_file(&drop_text);

    let t = app.lun.as_ref().unwrap().task_by_key("T-001").unwrap();
    assert!(t.notes.contains("[My File.pdf](file://"));
    assert!(t.notes.contains("[second.txt](file://"));
    assert!(t.notes.contains("[third.txt](file://"));
    assert!(t.notes.contains("My%20File.pdf"));
    assert!(
        !root.join(".lun/attachments").join("My File.pdf").exists(),
        "drop linking must not copy into .lun/attachments"
    );
    let links = app.lun.as_ref().unwrap().links_for_task(t.id).unwrap();
    assert!(links.iter().any(|l| l.uri.starts_with("file://")));

    let _ = std::fs::remove_dir_all(&root);
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
        View::Task,
        View::Log,
        View::Help,
        View::Placeholder,
    ] {
        app.view = view;
        let _ = screen(&app, 5, 3);
    }
    app.mode = Mode::Insert;
    app.notes_draft = "x".into();
    app.palette_open = true;
    app.palette_query = "status T-001".into();
    let _ = screen(&app, 5, 3);
}
