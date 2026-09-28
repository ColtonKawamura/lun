use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lun::cli::{run_result_in_reader, run_result_with_reader, App as CliApp};
use lun::tui::app::{App, CommandOutput, View};
use lun::tui::{data, render, term};
use lun::{Lun, ProjectSpec, TaskSpec};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::io::BufReader;
use std::path::PathBuf;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn prompt_reader(lines: &[&str]) -> BufReader<std::io::Cursor<Vec<u8>>> {
    let mut buf = Vec::new();
    for line in lines {
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
    }
    BufReader::new(std::io::Cursor::new(buf))
}

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("lun-p12-test-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture() -> (PathBuf, CliApp, App) {
    let root = temp_root("fx");
    let lun = Lun::init(&root).unwrap();
    let project = lun
        .create_project(ProjectSpec {
            name: "paper-stack".into(),
            ..Default::default()
        })
        .unwrap();
    lun.create_task(TaskSpec {
        title: "write methods".into(),
        project: Some(project.id),
        status: Some("doing".into()),
        priority: Some("med".into()),
        assignee: Some("me".into()),
        branch: None,
        labels: None,
        message: None,
        user: None,
    })
    .unwrap();
    let cli = CliApp::open(&root).unwrap();
    let lun = Lun::open(&root).unwrap();
    let snapshot = data::load(&lun, "0.1.0", "repo-path", "main".into(), Some("P-001")).unwrap();
    let tui = App::with_store(snapshot, root.clone(), lun);
    (root, cli, tui)
}

fn send_command(app: &mut App, text: &str) {
    term::handle_key(app, &key(KeyCode::Char('/')));
    for ch in text.chars() {
        term::handle_key(app, &key(KeyCode::Char(ch)));
    }
    term::handle_key(app, &key(KeyCode::Enter));
}

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

#[test]
fn tui_task_command_matches_cli_output_for_quoted_title() {
    let (root, cli, mut tui) = fixture();
    let mut input = prompt_reader(&[]);
    let cli_out =
        run_result_with_reader(&cli, &["task".into(), "write methods".into()], &mut input).unwrap();

    send_command(&mut tui, "task \"write methods\"");
    let tui_out = tui.output.as_ref().unwrap();
    assert!(!tui_out.is_error);
    assert_eq!(tui_out.text, cli_out);
    assert_eq!(tui.current_task().unwrap().task_key, "T-001");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_add_task_opens_native_form_with_prefilled_title_and_current_project() {
    let (root, _cli, mut tui) = fixture();
    send_command(&mut tui, "add task \"task from tui\"");
    assert!(tui.prompt_session.is_none());
    assert_eq!(tui.view, View::NewTask);
    if let Some(lun::tui::app::FormState::NewTask(draft)) = tui.form() {
        assert_eq!(draft.title, "task from tui");
        let project = &tui.data.projects[draft.project_index];
        assert_eq!(project.project_key, "P-001");
    } else {
        panic!("expected new-task form");
    }
    for _ in 0..8 {
        term::handle_key(&mut tui, &key(KeyCode::Enter));
    }
    assert_eq!(tui.view, View::Task);
    let task = tui.lun.as_ref().unwrap().task_by_key("T-002").unwrap();
    let project = tui.lun.as_ref().unwrap().project_by_key("P-001").unwrap();
    assert_eq!(task.project_id, Some(project.id));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_add_task_with_project_argument_prefills_selected_project() {
    let (root, _cli, mut tui) = fixture();
    send_command(&mut tui, "add task \"task from tui\" proj P-000");
    assert_eq!(tui.view, View::NewTask);
    if let Some(lun::tui::app::FormState::NewTask(draft)) = tui.form() {
        let project = &tui.data.projects[draft.project_index];
        assert_eq!(project.project_key, "P-000");
    } else {
        panic!("expected new-task form");
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_log_without_subject_uses_current_task_in_task_view() {
    let (root, cli, mut tui) = fixture();
    tui.view = View::Task;
    let mut input = prompt_reader(&[]);
    let cli_out =
        run_result_with_reader(&cli, &["log".into(), "T-001".into()], &mut input).unwrap();

    send_command(&mut tui, "log");
    let tui_out = tui.output.as_ref().unwrap();
    assert_eq!(tui_out.text, cli_out);
    assert!(!tui_out.is_error);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_retains_last_output_while_editing_canceling_and_empty_submitting() {
    let (root, _cli, mut tui) = fixture();
    send_command(&mut tui, "status");
    let first = tui.output.as_ref().unwrap().text.clone();

    term::handle_key(&mut tui, &key(KeyCode::Char('/')));
    for ch in "task T-001".chars() {
        term::handle_key(&mut tui, &key(KeyCode::Char(ch)));
    }
    assert_eq!(tui.output.as_ref().unwrap().text, first);
    term::handle_key(&mut tui, &key(KeyCode::Esc));
    assert_eq!(tui.output.as_ref().unwrap().text, first);

    term::handle_key(&mut tui, &key(KeyCode::Char('/')));
    term::handle_key(&mut tui, &key(KeyCode::Enter));
    assert_eq!(tui.output.as_ref().unwrap().text, first);

    let short = screen(&tui, 80, 10);
    let tall = screen(&tui, 80, 24);
    assert!(short.contains("Projects"));
    assert!(tall.contains("Projects"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_replaces_output_and_supports_scrolling() {
    let (root, _cli, mut tui) = fixture();
    tui.output = Some(CommandOutput {
        command: "demo".into(),
        text: (1..=20)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n"),
        is_error: false,
    });
    tui.view = View::Output;
    assert_eq!(tui.output_scroll, 0);
    term::handle_key(&mut tui, &key(KeyCode::PageDown));
    assert!(tui.output_scroll > 0);

    send_command(&mut tui, "task T-001");
    let out = tui.output.as_ref().unwrap();
    assert!(out.text.contains("Task T-001"));
    assert_eq!(tui.output_scroll, 0);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_attach_project_file_with_spaces_and_open_it() {
    let (root, _cli, mut tui) = fixture();
    let file = root.join("road map.md");
    std::fs::write(&file, "# roadmap\n").unwrap();

    let quoted = format!("attach project P-001 \"{}\"", file.display());
    send_command(&mut tui, &quoted);
    let tui_attach = tui.output.as_ref().unwrap().text.clone();

    let (root2, cli2, _tui2) = fixture();
    let file2 = root2.join("road map.md");
    std::fs::write(&file2, "# roadmap\n").unwrap();
    let mut input = prompt_reader(&[]);
    let cli_attach = run_result_in_reader(
        &cli2,
        &[
            "attach".into(),
            "project".into(),
            "P-001".into(),
            file2.display().to_string(),
        ],
        &mut input,
        Some(&root2),
    )
    .unwrap();
    assert!(tui_attach.starts_with("Attached road map.md to P-001"));
    assert!(cli_attach.starts_with("Attached road map.md to P-001"));

    std::env::set_var("LUN_OPEN_BIN", "true");
    send_command(&mut tui, "attach open project P-001 \"road map.md\"");
    let out = tui.output.as_ref().unwrap();
    assert!(out.text.contains("Opened:"));
    std::env::remove_var("LUN_OPEN_BIN");

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&root2);
}

#[test]
fn tui_external_attach_decline_surfaces_visible_error() {
    let (root, _cli, mut tui) = fixture();
    send_command(&mut tui, "status");
    let previous = tui.output.as_ref().unwrap().text.clone();

    let outside = root.parent().unwrap().join(format!(
        "outside-attach-{}-{}.txt",
        std::process::id(),
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&outside, "external").unwrap();

    send_command(
        &mut tui,
        &format!("attach task T-001 \"{}\"", outside.display()),
    );
    assert!(tui.prompt_session.is_some());
    for ch in "n".chars() {
        term::handle_key(&mut tui, &key(KeyCode::Char(ch)));
    }
    term::handle_key(&mut tui, &key(KeyCode::Enter));
    let out = tui.output.as_ref().unwrap();
    assert!(out.is_error);
    assert!(out.text.contains("not attached"));
    assert_ne!(out.text, previous);

    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_external_attach_prompt_accepts_shifted_confirmation() {
    let (root, _cli, mut tui) = fixture();
    let outside = root.parent().unwrap().join(format!(
        "outside-attach-yes-{}-{}.txt",
        std::process::id(),
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&outside, "external").unwrap();

    send_command(
        &mut tui,
        &format!("attach task T-001 \"{}\"", outside.display()),
    );
    assert!(tui.prompt_session.is_some());
    term::handle_key(
        &mut tui,
        &key_with_modifiers(KeyCode::Char('Y'), KeyModifiers::SHIFT),
    );
    term::handle_key(&mut tui, &key(KeyCode::Enter));
    let out = tui.output.as_ref().unwrap();
    assert!(!out.is_error);
    assert!(out.text.contains("Attached"));

    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&root);
}
