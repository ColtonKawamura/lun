//! Phase 4 tests: attachments & links (Mac linking).
//!
//! Covers the plan's Phase 4 flows: attaching in-repo files (real copies in
//! `.lun/attachments/`, collision suffixes), out-of-repo files with the
//! `y/N` confirmation (yes -> linked by path, no -> error, no copy), task and
//! project links, open-link lookup, ATTACH/LINK entries in `lun log`, and
//! the Attachments/Links sections of the `lun task` view.

use lun::cli::{
    add_link_command, attach_file, attachments_root, log_view, resolve_link, task_view, App,
};
use lun::{Lun, ProjectSpec, TaskSpec};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

fn prompt_reader(lines: &[&str]) -> BufReader<std::io::Cursor<Vec<u8>>> {
    let mut buf: Vec<u8> = Vec::new();
    for l in lines {
        buf.extend_from_slice(l.as_bytes());
        buf.push(b'\n');
    }
    BufReader::new(std::io::Cursor::new(buf))
}

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lun-p4-test-{name}-{}-{}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Fresh root with one task T-001 in project `p1` (plus the P-000 seed).
fn fixture() -> (PathBuf, App) {
    let root = temp_root("fx");
    let lun = Lun::init(&root).unwrap();
    lun.create_project(ProjectSpec {
        name: "p1".into(),
        ..Default::default()
    })
    .unwrap();
    lun.create_task(TaskSpec {
        title: "task one".into(),
        project: None,
        status: Some("todo".into()),
        priority: Some("med".into()),
        assignee: Some("me".into()),
        branch: None,
        labels: None,
        message: None,
        user: None,
    })
    .unwrap();
    (root, App { lun })
}

// ---------------------------------------------------------------------------
// lun attach task
// ---------------------------------------------------------------------------

#[test]
fn attach_in_repo_file_copies_it() {
    let (root, app) = fixture();
    let src = root.join("notes.txt");
    std::fs::write(&src, "hello attachment").unwrap();

    let mut input = prompt_reader(&[]);
    let out = attach_file(&app, &root, "T-001", src.to_str().unwrap(), &mut input).unwrap();

    assert!(out.starts_with("Attached notes.txt to T-001"), "{out}");
    let stored = root.join(".lun/attachments/notes.txt");
    assert!(stored.is_file(), "copy must land in .lun/attachments/");
    assert_eq!(
        std::fs::read_to_string(&stored).unwrap(),
        "hello attachment"
    );

    let rows = app.lun.attachments_for_task(app.lun.task_by_key("T-001").unwrap().id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].filename, "notes.txt");
    assert_eq!(rows[0].stored_path, stored.to_str().unwrap());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn attach_collision_gets_suffix() {
    let (root, app) = fixture();
    let src = root.join("dup.txt");
    std::fs::write(&src, "first").unwrap();
    let src2 = root.join("sub").join("dup.txt");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(&src2, "second").unwrap();

    let mut input = prompt_reader(&[]);
    attach_file(&app, &root, "T-001", src.to_str().unwrap(), &mut input).unwrap();
    let out = attach_file(&app, &root, "T-001", src2.to_str().unwrap(), &mut input).unwrap();

    assert!(out.contains("dup-2.txt"), "collision must be suffixed: {out}");
    let rows = app.lun.attachments_for_task(app.lun.task_by_key("T-001").unwrap().id).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].filename, "dup.txt");
    assert_eq!(rows[1].filename, "dup-2.txt");
    assert_eq!(
        std::fs::read_to_string(root.join(".lun/attachments/dup-2.txt")).unwrap(),
        "second"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn attach_out_of_repo_yes_links_by_path_without_copying() {
    let (root, app) = fixture();
    // A file outside the lun root (sibling dir).
    let outside = root.parent().unwrap().join(format!(
        "outside-{}-{}.txt",
        std::process::id(),
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&outside, "external data").unwrap();

    let mut input = prompt_reader(&["y"]);
    let out = attach_file(&app, &root, "T-001", outside.to_str().unwrap(), &mut input).unwrap();

    assert!(out.starts_with("Attached outside-"), "{out}");
    // No copy made into .lun/attachments/.
    assert!(
        !attachments_root(&root).join(outside.file_name().unwrap()).exists(),
        "out-of-repo file must NOT be copied"
    );
    let rows = app.lun.attachments_for_task(app.lun.task_by_key("T-001").unwrap().id).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].stored_path.starts_with(std::path::absolute(&outside).unwrap().to_str().unwrap()),
        "stored path must be the absolute external path: {}",
        rows[0].stored_path
    );
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn attach_out_of_repo_no_aborts() {
    let (root, app) = fixture();
    let outside = root.parent().unwrap().join(format!(
        "outside-no-{}-{}.txt",
        std::process::id(),
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&outside, "external data").unwrap();

    let mut input = prompt_reader(&["n"]);
    let e = attach_file(&app, &root, "T-001", outside.to_str().unwrap(), &mut input).unwrap_err();
    assert_eq!(e.kind(), "declined");
    assert!(e.to_string().contains("outside"), "{}", e.to_string());

    // Nothing recorded, nothing copied.
    let rows = app.lun.attachments_for_task(app.lun.task_by_key("T-001").unwrap().id).unwrap();
    assert!(rows.is_empty());
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn attach_missing_file_errors() {
    let (root, app) = fixture();
    let mut input = prompt_reader(&[]);
    let e = attach_file(&app, &root, "T-001", "/no/such/file-xyz.txt", &mut input).unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn attach_unknown_task_errors() {
    let (root, app) = fixture();
    let src = root.join("a.txt");
    std::fs::write(&src, "x").unwrap();
    let mut input = prompt_reader(&[]);
    let e = attach_file(&app, &root, "T-999", src.to_str().unwrap(), &mut input).unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// lun link
// ---------------------------------------------------------------------------

#[test]
fn link_task_and_project() {
    let (root, app) = fixture();
    let uri = "obsidian://open?vault=personal&file=testing%20bar-impact";
    let out = add_link_command(&app, "task", "T-001", "obsidian", uri).unwrap();
    assert_eq!(out, format!("Linked obsidian to task T-001: {uri}"));

    let out = add_link_command(&app, "project", "p1", "docs", "https://example.com/p1").unwrap();
    assert!(out.starts_with("Linked docs to project p1 [P-001]"), "{out}");

    let task_links = app
        .lun
        .links_for_task(app.lun.task_by_key("T-001").unwrap().id)
        .unwrap();
    assert_eq!(task_links.len(), 1);
    assert_eq!(task_links[0].label, "obsidian");
    assert_eq!(task_links[0].uri, uri);

    let proj_links = app
        .lun
        .links_for_project(app.lun.project_by_key("P-001").unwrap().id)
        .unwrap();
    assert_eq!(proj_links.len(), 1);
    assert_eq!(proj_links[0].uri, "https://example.com/p1");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn link_validation_and_errors() {
    let (root, app) = fixture();
    let e = add_link_command(&app, "task", "T-001", "", "http://x").unwrap_err();
    assert_eq!(e.kind(), "usage");
    let e = add_link_command(&app, "task", "T-001", "l", "  ").unwrap_err();
    assert_eq!(e.kind(), "usage");
    let e = add_link_command(&app, "bogus", "T-001", "l", "http://x").unwrap_err();
    assert_eq!(e.kind(), "usage");
    let e = add_link_command(&app, "task", "no-such", "l", "http://x").unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// lun open-link (lookup only — no real `open` spawn)
// ---------------------------------------------------------------------------

#[test]
fn resolve_link_returns_uri_for_task_and_project() {
    let (root, app) = fixture();
    add_link_command(&app, "task", "T-001", "obsidian", "obsidian://open?vault=p").unwrap();
    add_link_command(&app, "project", "P-001", "site", "https://example.org").unwrap();

    assert_eq!(
        resolve_link(&app, "task", "T-001", "obsidian").unwrap(),
        "obsidian://open?vault=p"
    );
    assert_eq!(
        resolve_link(&app, "project", "p1", "site").unwrap(),
        "https://example.org"
    );

    let e = resolve_link(&app, "task", "T-001", "nope").unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let e = resolve_link(&app, "project", "P-999", "site").unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let e = resolve_link(&app, "bogus", "T-001", "obsidian").unwrap_err();
    assert_eq!(e.kind(), "usage");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn duplicate_link_labels_are_ambiguous() {
    let (root, app) = fixture();
    add_link_command(&app, "task", "T-001", "ref", "https://a").unwrap();
    add_link_command(&app, "task", "T-001", "ref", "https://b").unwrap();
    let e = resolve_link(&app, "task", "T-001", "ref").unwrap_err();
    assert_eq!(e.kind(), "ambiguous");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// log + task view integration
// ---------------------------------------------------------------------------

#[test]
fn attach_and_link_appear_in_task_log() {
    let (root, app) = fixture();
    let src = root.join("fig.png");
    std::fs::write(&src, "png-bytes").unwrap();
    let mut input = prompt_reader(&[]);
    attach_file(&app, &root, "T-001", src.to_str().unwrap(), &mut input).unwrap();
    add_link_command(&app, "task", "T-001", "obsidian", "obsidian://open?vault=p").unwrap();

    let out = log_view(&app, "T-001").unwrap();
    assert!(out.contains("ATTACH"), "{out}");
    assert!(out.contains("LINK"), "{out}");
    assert!(out.contains("File: fig.png"), "{out}");
    assert!(out.contains("Link: [obsidian] obsidian://open?vault=p"), "{out}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn task_view_shows_attachments_and_links() {
    let (root, app) = fixture();
    let src = root.join("data.csv");
    std::fs::write(&src, "a,b\n1,2\n").unwrap();
    let mut input = prompt_reader(&[]);
    attach_file(&app, &root, "T-001", src.to_str().unwrap(), &mut input).unwrap();
    add_link_command(&app, "task", "T-001", "wiki", "https://wiki.example/t1").unwrap();

    let out = task_view(&app, "T-001").unwrap();
    assert!(out.contains("\nAttachments:\n"), "{out}");
    assert!(out.contains("- data.csv ("), "{out}");
    assert!(out.contains(".lun/attachments/data.csv"), "{out}");
    assert!(out.contains("\nLinks:\n"), "{out}");
    assert!(out.contains("- [wiki] https://wiki.example/t1"), "{out}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn task_view_empty_attachment_and_link_stubs() {
    let (root, app) = fixture();
    let out = task_view(&app, "T-001").unwrap();
    assert!(out.contains("Attachments:\n- (no attachments"), "{out}");
    assert!(out.contains("Links:\n- (no links"), "{out}");
    let _ = std::fs::remove_dir_all(&root);
}
