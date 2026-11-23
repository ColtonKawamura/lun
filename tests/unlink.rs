//! `lun unlink`: remove a link by label or uri, keep the UNLINK log entry,
//! refuse to guess when several links match, and let a handover swap links.

use lun::cli::{complete_output, help_text, run_result_in_reader, App};
use lun::{LinkTarget, Lun, TaskSpec};
use std::io::Cursor;
use std::path::PathBuf;

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lun-unlink-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A board with one task; returns the App (for CLI calls) and the task key.
fn fixture(name: &str) -> (App, String, i64) {
    let root = temp_root(name);
    let _ = Lun::init(&root).unwrap();
    let app = App::open(&root).unwrap();
    let t = app
        .lun
        .create_task(TaskSpec {
            title: "card".into(),
            project: None,
            status: Some("planning".into()),
            priority: Some("med".into()),
            assignee: Some("coder".into()),
            branch: None,
            labels: None,
            message: None,
            scope: None,
            user: None,
        })
        .unwrap();
    (app, t.task_key, t.id)
}

fn cli(app: &App, args: &[&str]) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut stdin = Cursor::new(Vec::<u8>::new());
    run_result_in_reader(app, &args, &mut stdin, None).map_err(|e| e.to_string())
}

#[test]
fn unlink_one_match_removes_it_and_logs_unlink() {
    let (app, key, tid) = fixture("one");
    app.lun
        .add_link(LinkTarget::Task(tid), "chat session", "hermes://open/aaa", None, None)
        .unwrap();

    let out = cli(&app, &["unlink", "task", &key, "chat session"]).unwrap();
    assert!(out.starts_with("Unlinked \"chat session\" (hermes://open/aaa) from "), "{out}");
    assert!(app.lun.links_for_task(tid).unwrap().is_empty());
    let logs = app.lun.logs_for("task", tid).unwrap();
    assert_eq!(logs[0].action, "UNLINK");
    assert!(logs[0].details.contains("hermes://open/aaa"));
}

#[test]
fn unlink_by_uri_picks_one_of_several_same_labels() {
    let (app, key, tid) = fixture("uri");
    app.lun
        .add_link(LinkTarget::Task(tid), "chat session", "hermes://open/old", None, None)
        .unwrap();
    app.lun
        .add_link(LinkTarget::Task(tid), "chat session", "hermes://open/new", None, None)
        .unwrap();

    cli(&app, &["unlink", "task", &key, "hermes://open/old"]).unwrap();
    let left = app.lun.links_for_task(tid).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].uri, "hermes://open/new");
}

#[test]
fn several_matches_without_all_are_refused_and_nothing_is_removed() {
    let (app, key, tid) = fixture("ambig");
    app.lun
        .add_link(LinkTarget::Task(tid), "chat session", "hermes://open/a", None, None)
        .unwrap();
    app.lun
        .add_link(LinkTarget::Task(tid), "chat session", "hermes://open/b", None, None)
        .unwrap();

    let err = cli(&app, &["unlink", "task", &key, "chat session"]).unwrap_err();
    assert!(err.contains("2 links match 'chat session'"), "{err}");
    assert!(err.contains("hermes://open/a") && err.contains("hermes://open/b"), "{err}");
    assert_eq!(app.lun.links_for_task(tid).unwrap().len(), 2);
}

#[test]
fn all_removes_every_match_and_restore_brings_back_newest() {
    let (app, key, tid) = fixture("all");
    app.lun
        .add_link(LinkTarget::Task(tid), "chat session", "hermes://open/a", None, None)
        .unwrap();
    app.lun
        .add_link(LinkTarget::Task(tid), "chat session", "hermes://open/b", None, None)
        .unwrap();

    let out = cli(&app, &["unlink", "task", &key, "chat session", "--all"]).unwrap();
    assert!(out.starts_with("Unlinked 2 links from"), "{out}");
    assert!(app.lun.links_for_task(tid).unwrap().is_empty());

    let restored = app.lun.restore_last_link(LinkTarget::Task(tid), None).unwrap().unwrap();
    assert_eq!(restored.uri, "hermes://open/b");
}

#[test]
fn no_match_is_not_found_and_bad_usage_is_rejected() {
    let (app, key, _tid) = fixture("misc");
    let err = cli(&app, &["unlink", "task", &key, "nope"]).unwrap_err();
    assert!(err.contains("no link with label or uri 'nope'"), "{err}");

    let err = cli(&app, &["unlink", "task", &key]).unwrap_err();
    assert!(err.contains("expected: lun unlink"), "{err}");

    let err = cli(&app, &["unlink", "widget", &key, "x"]).unwrap_err();
    assert!(err.contains("kind must be 'task' or 'project'"), "{err}");

    let err = cli(&app, &["unlink", "task", &key, "x", "--force"]).unwrap_err();
    assert!(err.contains("unexpected flag '--force'"), "{err}");
}

#[test]
fn handover_swaps_the_old_session_link_for_the_new_one() {
    let (app, key, tid) = fixture("handover");
    cli(&app, &["link", "task", &key, "chat session", "hermes://open/previous"]).unwrap();

    cli(&app, &["unlink", "task", &key, "hermes://open/previous"]).unwrap();
    cli(&app, &["link", "task", &key, "chat session", "hermes://open/current"]).unwrap();

    let links = app.lun.links_for_task(tid).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].uri, "hermes://open/current");
}

#[test]
fn unlink_is_listed_in_help_and_completion() {
    let (app, _key, _tid) = fixture("help");
    assert!(help_text("0").contains("lun unlink <task|project> <key|title> <label|uri> [--all]"));
    let out = complete_output(Some(&app), &["--".into(), "lun".into(), "unl".into()]);
    assert!(out.lines().any(|l| l.trim() == "unlink"), "{out}");
}
