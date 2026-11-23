//! `lun rm proj`: delete a project from the DB layer, the CLI, the TUI and
//! the web API. Covers: by name and by key, refusing P-000, refusing a
//! project that still has tasks, --move-to, --cascade, the y/N confirm,
//! child rows (logs, links, attachments, PRs, note diffs), key reuse, and
//! that nothing else on the board is touched.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lun::cli::{run_result_in_reader, App as CliApp};
use lun::tui::app::{App as TuiApp, View};
use lun::tui::{data, term};
use lun::{
    AttachmentTarget, LinkTarget, Lun, PrSpec, ProjectDeleteMode, ProjectSpec, TaskSpec,
    TaskUpdateSpec,
};
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lun-projdel-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn reader(lines: &[&str]) -> BufReader<std::io::Cursor<Vec<u8>>> {
    let mut buf = Vec::new();
    for l in lines {
        buf.extend_from_slice(l.as_bytes());
        buf.push(b'\n');
    }
    BufReader::new(std::io::Cursor::new(buf))
}

fn mk_task(lun: &Lun, title: &str, project: Option<i64>) -> lun::Task {
    lun.create_task(TaskSpec {
        title: title.into(),
        project,
        status: Some("planning".into()),
        priority: Some("med".into()),
        assignee: Some("me".into()),
        branch: Some("feat".into()),
        labels: None,
        message: None,
        scope: None,
        user: None,
    })
    .unwrap()
}

fn mk_proj(lun: &Lun, name: &str) -> lun::Project {
    lun.create_project(ProjectSpec {
        name: name.into(),
        ..Default::default()
    })
    .unwrap()
}

fn count(root: &std::path::Path, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn rm(cli: &CliApp, root: &std::path::Path, args: &[&str], input: &[&str]) -> lun::db::Result<String> {
    let mut argv: Vec<String> = vec!["rm".into(), "proj".into()];
    argv.extend(args.iter().map(|s| s.to_string()));
    run_result_in_reader(cli, &argv, &mut reader(input), Some(root))
}

// ---------------------------------------------------------------------------
// DB layer
// ---------------------------------------------------------------------------

#[test]
fn db_deletes_empty_project_and_its_own_rows() {
    let root = temp_root("db-empty");
    let lun = Lun::init(&root).unwrap();
    let p = mk_proj(&lun, "scratch");
    lun.add_link(LinkTarget::Project(p.id), "doc", "https://x", None, None).unwrap();
    lun.add_attachment_to(AttachmentTarget::Project(p.id), "a.txt", "/tmp/nope-a.txt", None, None).unwrap();
    let other = mk_proj(&lun, "keeper");

    let done = lun
        .delete_project(p.id, ProjectDeleteMode::RefuseIfNotEmpty, None, Some("t"))
        .unwrap();
    assert_eq!(done.project.name, "scratch");
    assert_eq!((done.tasks_moved, done.tasks_deleted), (0, 0));
    assert_eq!(done.attachment_paths, vec!["/tmp/nope-a.txt".to_string()]);

    assert!(lun.project_by_key(&p.project_key).is_err());
    assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM links WHERE project_id = {}", p.id)), 0);
    assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM attachments WHERE project_id = {}", p.id)), 0);
    assert_eq!(
        count(&root, &format!("SELECT COUNT(*) FROM logs WHERE entity_type='project' AND entity_id={}", p.id)),
        0
    );
    // Untouched.
    assert!(lun.project_by_key(&other.project_key).is_ok());
    // The deletion leaves a DELETE entry on P-000 with the details.
    let p0 = lun.unassigned_project_id().unwrap();
    let last = &lun.logs_for("project", p0).unwrap()[0];
    assert_eq!(last.action, "DELETE");
    assert!(last.details.contains(&p.project_key), "{}", last.details);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn db_refuses_p000_missing_and_nonempty() {
    let root = temp_root("db-refuse");
    let lun = Lun::init(&root).unwrap();
    let p0 = lun.unassigned_project_id().unwrap();
    let e = lun.delete_project(p0, ProjectDeleteMode::Cascade, None, None).unwrap_err();
    assert_eq!(e.kind(), "invalid");
    assert!(lun.project_by_key("P-000").is_ok());

    let e = lun.delete_project(99999, ProjectDeleteMode::Cascade, None, None).unwrap_err();
    assert_eq!(e.kind(), "not-found");

    let p = mk_proj(&lun, "busy");
    mk_task(&lun, "t1", Some(p.id));
    let e = lun
        .delete_project(p.id, ProjectDeleteMode::RefuseIfNotEmpty, None, None)
        .unwrap_err();
    assert_eq!(e.kind(), "invalid");
    assert!(e.to_string().contains("1 task"), "{e}");
    // Nothing changed.
    assert!(lun.project_by_key(&p.project_key).is_ok());
    assert_eq!(lun.project_task_count(p.id).unwrap(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn db_move_to_keeps_tasks_and_logs_the_move() {
    let root = temp_root("db-move");
    let lun = Lun::init(&root).unwrap();
    let a = mk_proj(&lun, "from");
    let b = mk_proj(&lun, "to");
    let t1 = mk_task(&lun, "one", Some(a.id));
    let t2 = mk_task(&lun, "two", Some(a.id));
    lun.add_link(LinkTarget::Task(t1.id), "l", "https://t1", None, None).unwrap();

    // Same project is refused, and nothing is deleted.
    let e = lun.delete_project(a.id, ProjectDeleteMode::MoveTo(a.id), None, None).unwrap_err();
    assert_eq!(e.kind(), "invalid");
    // Missing target is refused, and nothing is deleted.
    let e = lun.delete_project(a.id, ProjectDeleteMode::MoveTo(99999), None, None).unwrap_err();
    assert_eq!(e.kind(), "not-found");
    assert_eq!(lun.project_task_count(a.id).unwrap(), 2);

    let done = lun.delete_project(a.id, ProjectDeleteMode::MoveTo(b.id), None, None).unwrap();
    assert_eq!((done.tasks_moved, done.tasks_deleted), (2, 0));
    assert_eq!(done.moved_to.unwrap().project_key, b.project_key);
    assert!(lun.project_by_key(&a.project_key).is_err());
    assert_eq!(lun.task_by_id(t1.id).unwrap().project_id, Some(b.id));
    assert_eq!(lun.task_by_id(t2.id).unwrap().project_id, Some(b.id));
    // Task history, links survive; the move is logged on the task.
    assert_eq!(lun.links_for_task(t1.id).unwrap().len(), 1);
    let log = &lun.logs_for("task", t1.id).unwrap()[0];
    assert_eq!(log.action, "UPDATE");
    assert!(log.message.contains("move"), "{}", log.message);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn db_cascade_removes_task_children_without_fk_errors() {
    let root = temp_root("db-cascade");
    let lun = Lun::init(&root).unwrap();
    let doomed = mk_proj(&lun, "doomed");
    let safe = mk_proj(&lun, "safe");
    let t = mk_task(&lun, "in doomed", Some(doomed.id));
    let s = mk_task(&lun, "in safe", Some(safe.id));
    lun.add_link(LinkTarget::Task(t.id), "l", "https://t", None, None).unwrap();
    lun.add_attachment_to(AttachmentTarget::Task(t.id), "f.txt", "/tmp/nope-f.txt", None, None).unwrap();
    lun.create_pr(PrSpec {
        task_id: t.id,
        source_branch: Some("feat".into()),
        target_branch: Some("main".into()),
        message: None,
        user: None,
    })
    .unwrap();
    // Two note edits so note_diffs has rows for the doomed task.
    lun.update_task(t.id, TaskUpdateSpec { notes: Some("one".into()), ..Default::default() }).unwrap();
    lun.update_task(t.id, TaskUpdateSpec { notes: Some("two".into()), ..Default::default() }).unwrap();
    assert!(count(&root, &format!("SELECT COUNT(*) FROM note_diffs WHERE task_id={}", t.id)) >= 1);
    // Safe task has the same kinds of rows and must keep all of them.
    lun.add_link(LinkTarget::Task(s.id), "l", "https://s", None, None).unwrap();
    lun.update_task(s.id, TaskUpdateSpec { notes: Some("keep".into()), ..Default::default() }).unwrap();
    let safe_logs = count(&root, &format!("SELECT COUNT(*) FROM logs WHERE entity_type='task' AND entity_id={}", s.id));

    let done = lun.delete_project(doomed.id, ProjectDeleteMode::Cascade, None, None).unwrap();
    assert_eq!((done.tasks_moved, done.tasks_deleted), (0, 1));
    assert_eq!(done.attachment_paths, vec!["/tmp/nope-f.txt".to_string()]);

    for (table, col) in [
        ("links", "task_id"),
        ("attachments", "task_id"),
        ("prs", "task_id"),
        ("note_diffs", "task_id"),
    ] {
        assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM {table} WHERE {col}={}", t.id)), 0, "{table}");
    }
    assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM tasks WHERE id={}", t.id)), 0);
    assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM logs WHERE entity_type='task' AND entity_id={}", t.id)), 0);
    // The other project's task is intact, with its own rows.
    assert!(lun.task_by_id(s.id).is_ok());
    assert_eq!(lun.links_for_task(s.id).unwrap().len(), 1);
    assert_eq!(
        count(&root, &format!("SELECT COUNT(*) FROM logs WHERE entity_type='task' AND entity_id={}", s.id)),
        safe_logs
    );
    {
        let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
        let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
        let violations = stmt.query_map([], |_| Ok(())).unwrap().count();
        assert_eq!(violations, 0, "dangling foreign keys after cascade");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn db_cascade_includes_archived_tasks() {
    let root = temp_root("db-archived");
    let lun = Lun::init(&root).unwrap();
    let p = mk_proj(&lun, "withold");
    let t = mk_task(&lun, "old", Some(p.id));
    lun.archive_task(t.id, None, None).unwrap();
    // An archived task still blocks a plain delete: it is a real row.
    assert_eq!(lun.project_task_count(p.id).unwrap(), 1);
    assert!(lun.delete_project(p.id, ProjectDeleteMode::RefuseIfNotEmpty, None, None).is_err());
    lun.delete_project(p.id, ProjectDeleteMode::Cascade, None, None).unwrap();
    assert_eq!(count(&root, "SELECT COUNT(*) FROM tasks"), 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn db_log_hash_chain_still_valid_after_delete() {
    let root = temp_root("db-hash");
    let lun = Lun::init(&root).unwrap();
    let p = mk_proj(&lun, "gone");
    lun.delete_project(p.id, ProjectDeleteMode::RefuseIfNotEmpty, None, None).unwrap();
    // A later entry on P-000 chains onto the DELETE entry, and a new
    // project reusing no key still logs fine.
    let q = mk_proj(&lun, "after");
    assert_ne!(q.project_key, p.project_key);
    let p0 = lun.unassigned_project_id().unwrap();
    lun.log("project", p0, "COMMENT", "after delete", "{}", None).unwrap();
    let logs = lun.logs_for("project", p0).unwrap();
    assert!(logs.iter().all(|l| l.hash.len() == 64));
    let mut hashes: Vec<_> = logs.iter().map(|l| l.hash.clone()).collect();
    hashes.sort();
    hashes.dedup();
    assert_eq!(hashes.len(), logs.len());
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// CLI (in-process)
// ---------------------------------------------------------------------------

fn cli_fixture(name: &str) -> (PathBuf, CliApp, Lun) {
    let root = temp_root(name);
    let lun = Lun::init(&root).unwrap();
    let cli = CliApp::open(&root).unwrap();
    (root, cli, lun)
}

#[test]
fn cli_rm_by_name_and_by_key() {
    let (root, cli, lun) = cli_fixture("cli-name-key");
    let a = mk_proj(&lun, "alpha site");
    let b = mk_proj(&lun, "beta");
    let out = rm(&cli, &root, &["alpha site", "--yes", "--message", "bye alpha"], &[]).unwrap();
    assert!(out.contains("Deleted project alpha site"), "{out}");
    assert!(out.contains("Committed: bye alpha"), "{out}");
    assert!(lun.project_by_key(&a.project_key).is_err());

    let out = rm(&cli, &root, &[&b.project_key, "--yes"], &[""]).unwrap();
    assert!(out.contains(&b.project_key), "{out}");
    assert!(lun.project_by_key(&b.project_key).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rm_confirm_prompt_yes_no_and_default() {
    let (root, cli, lun) = cli_fixture("cli-confirm");
    let p = mk_proj(&lun, "askme");

    // Empty answer = no.
    let e = rm(&cli, &root, &["askme"], &[""]).unwrap_err();
    assert_eq!(e.kind(), "declined");
    assert!(lun.project_by_key(&p.project_key).is_ok());
    // "n" = no.
    let e = rm(&cli, &root, &["askme"], &["n"]).unwrap_err();
    assert_eq!(e.kind(), "declined");
    // Closed stdin = no.
    let e = rm(&cli, &root, &["askme"], &[]).unwrap_err();
    assert_eq!(e.kind(), "declined");
    assert!(lun.project_by_key(&p.project_key).is_ok());
    // "y" then an empty commit message takes the default.
    let out = rm(&cli, &root, &["askme"], &["y", ""]).unwrap();
    assert!(out.contains("Committed: delete project \"askme\""), "{out}");
    assert!(lun.project_by_key(&p.project_key).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rm_errors_unknown_p000_taskkey_and_usage() {
    let (root, cli, lun) = cli_fixture("cli-errors");
    let p = mk_proj(&lun, "real");
    let t = mk_task(&lun, "a task", Some(p.id));

    let e = rm(&cli, &root, &["nope", "--yes"], &[]).unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let e = rm(&cli, &root, &["P-zzzz", "--yes"], &[]).unwrap_err();
    assert_eq!(e.kind(), "not-found");
    // A task key is not a project.
    let e = rm(&cli, &root, &[&t.task_key, "--yes"], &[]).unwrap_err();
    assert_eq!(e.kind(), "not-found");
    assert!(e.to_string().contains("task key"), "{e}");
    // P-000 by key and by name.
    for q in ["P-000", "Unassigned"] {
        let e = rm(&cli, &root, &[q, "--yes"], &[]).unwrap_err();
        assert_eq!(e.kind(), "invalid", "{q}");
        assert!(e.to_string().contains("cannot be deleted"), "{e}");
    }
    // Usage errors.
    for args in [
        vec![],
        vec!["a", "b"],
        vec!["real", "--bogus"],
        vec!["real", "--move-to"],
        vec!["real", "--move-to", "x", "--cascade"],
    ] {
        let e = rm(&cli, &root, &args, &[]).unwrap_err();
        assert_eq!(e.kind(), "usage", "{args:?}: {e}");
    }
    // `lun rm` and `lun rm task` have no meaning.
    let argv: Vec<String> = vec!["rm".into()];
    assert_eq!(run_result_in_reader(&cli, &argv, &mut reader(&[]), Some(&root)).unwrap_err().kind(), "usage");
    // Nothing was deleted by any of that.
    assert!(lun.project_by_key(&p.project_key).is_ok());
    assert!(lun.project_by_key("P-000").is_ok());
    assert!(lun.task_by_id(t.id).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rm_nonempty_refused_before_prompting_then_move_and_cascade() {
    let (root, cli, lun) = cli_fixture("cli-nonempty");
    let a = mk_proj(&lun, "full");
    let b = mk_proj(&lun, "dest");
    let t1 = mk_task(&lun, "x1", Some(a.id));
    mk_task(&lun, "x2", Some(a.id));

    // Refused (no flags), even with --yes.
    let e = rm(&cli, &root, &["full", "--yes"], &[]).unwrap_err();
    assert_eq!(e.kind(), "invalid");
    assert!(e.to_string().contains("--move-to") && e.to_string().contains("--cascade"), "{e}");
    assert!(e.to_string().contains("2 tasks"), "{e}");
    assert_eq!(lun.project_task_count(a.id).unwrap(), 2);

    // --move-to a missing / same project is refused.
    assert_eq!(rm(&cli, &root, &["full", "--move-to", "nope", "--yes"], &[]).unwrap_err().kind(), "not-found");
    assert_eq!(rm(&cli, &root, &["full", "--move-to", "full", "--yes"], &[]).unwrap_err().kind(), "invalid");
    assert_eq!(lun.project_task_count(a.id).unwrap(), 2);

    // Declining the prompt after --move-to changes nothing.
    let e = rm(&cli, &root, &["full", "--move-to", &b.project_key], &["n"]).unwrap_err();
    assert_eq!(e.kind(), "declined");
    assert_eq!(lun.project_task_count(a.id).unwrap(), 2);

    // --move-to by name works.
    let out = rm(&cli, &root, &["full", "--move-to", "dest", "--yes"], &[""]).unwrap();
    assert!(out.contains("Moved 2 tasks to dest"), "{out}");
    assert_eq!(lun.task_by_id(t1.id).unwrap().project_id, Some(b.id));
    assert!(lun.project_by_key(&a.project_key).is_err());

    // --cascade deletes tasks too.
    let c = mk_proj(&lun, "cascade me");
    let t3 = mk_task(&lun, "y1", Some(c.id));
    let out = rm(&cli, &root, &["cascade me", "--cascade", "--yes"], &[""]).unwrap();
    assert!(out.contains("Deleted 1 task"), "{out}");
    assert!(lun.task_by_id(t3.id).is_err());
    // dest and its tasks are untouched.
    assert_eq!(lun.project_task_count(b.id).unwrap(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rm_removes_managed_attachment_copies_only() {
    let (root, cli, lun) = cli_fixture("cli-attach");
    let p = mk_proj(&lun, "files");
    let managed_dir = root.join(".lun/attachments");
    std::fs::create_dir_all(&managed_dir).unwrap();
    let managed = managed_dir.join("kept-copy.txt");
    std::fs::write(&managed, "x").unwrap();
    let outside = root.join("linked-by-path.txt");
    std::fs::write(&outside, "y").unwrap();
    lun.add_attachment_to(AttachmentTarget::Project(p.id), "kept-copy.txt", &managed.display().to_string(), None, None).unwrap();
    lun.add_attachment_to(AttachmentTarget::Project(p.id), "linked-by-path.txt", &outside.display().to_string(), None, None).unwrap();

    rm(&cli, &root, &["files", "--yes"], &[""]).unwrap();
    assert!(!managed.exists(), "managed copy should be removed");
    assert!(outside.exists(), "a file linked by path must never be deleted");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rm_name_with_quote_and_unicode_and_key_reuse() {
    let (root, cli, lun) = cli_fixture("cli-names");
    let weird = mk_proj(&lun, "q\"uote ünï");
    rm(&cli, &root, &["q\"uote ünï", "--yes"], &[""]).unwrap();
    assert!(lun.project_by_key(&weird.project_key).is_err());
    // Same name can be created again after deleting, with a fresh key.
    let again = mk_proj(&lun, "q\"uote ünï");
    assert_ne!(again.project_key, weird.project_key);
    // Tasks added to the new one do not inherit the deleted project's log.
    let logs = lun.logs_for("project", again.id).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].action, "CREATE");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rm_alias_project_word() {
    let (root, cli, lun) = cli_fixture("cli-alias");
    let p = mk_proj(&lun, "aliased");
    let argv: Vec<String> = ["rm", "project", "aliased", "--yes"].iter().map(|s| s.to_string()).collect();
    run_result_in_reader(&cli, &argv, &mut reader(&[""]), Some(&root)).unwrap();
    assert!(lun.project_by_key(&p.project_key).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_completion_knows_rm() {
    let (root, cli, lun) = cli_fixture("cli-complete");
    let p = mk_proj(&lun, "completeme");
    let c = |words: &[&str]| {
        let w: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        lun::cli::complete_output(Some(&cli), &w)
    };
    assert!(c(&["lun", "r"]).lines().any(|l| l == "rm"), "{}", c(&["lun", "r"]));
    assert!(c(&["lun", "rm", ""]).lines().any(|l| l == "proj"));
    let names = c(&["lun", "rm", "proj", ""]);
    assert!(names.lines().any(|l| l == p.project_key) && names.lines().any(|l| l == "completeme"), "{names}");
    let flags = c(&["lun", "rm", "proj", "x", "--"]);
    assert!(flags.contains("--move-to") && flags.contains("--cascade") && flags.contains("--yes"), "{flags}");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// CLI (the real binary: exit codes, stdin closed, walk-up from a subdirectory)
// ---------------------------------------------------------------------------

#[test]
fn binary_exit_codes_and_closed_stdin() {
    // HOME is the walk-up ceiling and is never itself searched from below,
    // so the board lives in a directory under it.
    let home = std::fs::canonicalize(temp_root("bin")).unwrap();
    let root = home.join("board");
    std::fs::create_dir_all(&root).unwrap();
    let bin = env!("CARGO_BIN_EXE_lun");
    let run = |dir: &std::path::Path, args: &[&str]| {
        let out = Command::new(bin)
            .args(args)
            .current_dir(dir)
            .env("HOME", &home)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    assert_eq!(run(&root, &["init"]).0, 0);
    let (code, out, err) = run(&root, &["add", "proj", "killme", "--message", "m"]);
    assert_eq!(code, 0, "{out}{err}");

    // Closed stdin and no --yes: declined, exit 2, project survives.
    let (code, _, err) = run(&root, &["rm", "proj", "killme"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("not deleted"), "{err}");
    assert!(run(&root, &["status", "killme"]).0 == 0);

    // P-000: exit 2.
    let (code, _, err) = run(&root, &["rm", "proj", "P-000", "--yes"]);
    assert_eq!(code, 2, "{err}");

    // From a subdirectory, with --yes: deleted, exit 0.
    let sub = root.join("deep/er");
    std::fs::create_dir_all(&sub).unwrap();
    let (code, out, err) = run(&sub, &["rm", "proj", "killme", "--yes", "--message", "bin test"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("Deleted project killme"), "{out}");
    assert_eq!(run(&root, &["status", "killme"]).0, 2);

    // Help mentions it.
    let (_, out, _) = run(&root, &["--help"]);
    assert!(out.contains("lun rm proj"), "{out}");
    let _ = std::fs::remove_dir_all(&home);
}

// ---------------------------------------------------------------------------
// TUI
// ---------------------------------------------------------------------------

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn tui_fixture(name: &str) -> (PathBuf, TuiApp, Vec<lun::Project>) {
    let root = temp_root(name);
    let lun = Lun::init(&root).unwrap();
    let projs = vec![mk_proj(&lun, "alpha"), mk_proj(&lun, "bravo"), mk_proj(&lun, "charlie")];
    mk_task(&lun, "bravo task", Some(projs[1].id));
    let d = data::load(&lun, "0.1.0", "repo", "main".into(), Some(&projs[0].project_key)).unwrap();
    let app = TuiApp::with_store(d, root.clone(), lun);
    (root, app, projs)
}

fn type_line(app: &mut TuiApp, text: &str) {
    for ch in text.chars() {
        term::handle_key(app, &key(KeyCode::Char(ch)));
    }
    term::handle_key(app, &key(KeyCode::Enter));
}

fn select_project(app: &mut TuiApp, key_str: &str) {
    app.enter_view(View::Project, "");
    let pos = app.data.projects.iter().position(|p| p.project_key == key_str).unwrap();
    app.project_selected = pos;
}

#[test]
fn tui_d_key_prefills_command_and_confirm_deletes() {
    let (root, mut app, projs) = tui_fixture("tui-d");
    select_project(&mut app, &projs[0].project_key);
    term::handle_key(&mut app, &KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT));
    assert!(app.palette_open);
    assert_eq!(app.palette_query, format!("rm proj {}", projs[0].project_key));
    // Enter -> confirm prompt, not an immediate delete.
    term::handle_key(&mut app, &key(KeyCode::Enter));
    let label = app.current_prompt_label().expect("confirm prompt");
    assert!(label.contains("[y/N]") && label.contains("alpha"), "{label}");
    assert!(app.data.projects.iter().any(|p| p.project_key == projs[0].project_key));
    type_line(&mut app, "y");
    // Then the commit message prompt (empty = default).
    assert!(app.current_prompt_label().unwrap().contains("Commit Message"));
    type_line(&mut app, "");
    let out = app.output.as_ref().unwrap();
    assert!(!out.is_error, "{}", out.text);
    assert!(out.text.contains("Deleted project alpha"), "{}", out.text);
    assert!(!app.data.projects.iter().any(|p| p.project_key == projs[0].project_key));
    assert!(app.project_selected < app.data.projects.len());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_no_answer_cancels_without_commit_prompt() {
    let (root, mut app, projs) = tui_fixture("tui-no");
    select_project(&mut app, &projs[2].project_key);
    term::handle_key(&mut app, &KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT));
    term::handle_key(&mut app, &key(KeyCode::Enter));
    type_line(&mut app, "n");
    assert!(app.prompt_session.is_none(), "no commit-message prompt after a no");
    let out = app.output.as_ref().unwrap();
    assert!(out.is_error && out.text.contains("not deleted"), "{}", out.text);
    assert!(app.data.projects.iter().any(|p| p.project_key == projs[2].project_key));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_nonempty_is_refused_with_hint_and_p000_blocked() {
    let (root, mut app, projs) = tui_fixture("tui-refuse");
    select_project(&mut app, &projs[1].project_key);
    term::handle_key(&mut app, &KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT));
    term::handle_key(&mut app, &key(KeyCode::Enter));
    // Refused up front: no prompt, an error message with the way out.
    assert!(app.prompt_session.is_none());
    let msg = app.message.as_ref().map(|m| m.0.clone()).unwrap_or_default();
    assert!(msg.contains("1 task") && msg.contains("--move-to") && msg.contains("--cascade"), "{msg}");
    assert!(app.data.projects.iter().any(|p| p.project_key == projs[1].project_key));

    // With --move-to it goes through the same prompts.
    app.close_command_prompt();
    app.open_palette();
    type_line(&mut app, &format!("rm proj {} --move-to {}", projs[1].project_key, projs[2].project_key));
    type_line(&mut app, "y");
    type_line(&mut app, "moved on");
    assert!(!app.output.as_ref().unwrap().is_error, "{}", app.output.as_ref().unwrap().text);
    assert!(!app.data.projects.iter().any(|p| p.project_key == projs[1].project_key));
    assert_eq!(app.data.tasks[0].project_id, Some(projs[2].id));

    // P-000: D refuses with a message, no palette.
    app.close_command_prompt();
    select_project(&mut app, "P-000");
    term::handle_key(&mut app, &KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT));
    assert!(!app.palette_open);
    assert!(app.message.as_ref().unwrap().0.contains("cannot be deleted"));
    assert!(app.data.projects.iter().any(|p| p.project_key == "P-000"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_selection_follows_project_by_key_after_delete_above_it() {
    let (root, mut app, projs) = tui_fixture("tui-sel");
    // Highlight charlie (last), delete alpha above it via the command line.
    select_project(&mut app, &projs[2].project_key);
    app.data.current_project = app.project_selected;
    app.open_palette();
    type_line(&mut app, &format!("rm proj {} --yes", projs[0].project_key));
    type_line(&mut app, "");
    assert!(!app.output.as_ref().unwrap().is_error, "{}", app.output.as_ref().unwrap().text);
    assert_eq!(app.data.projects[app.project_selected].project_key, projs[2].project_key);
    assert_eq!(app.data.current().unwrap().project_key, projs[2].project_key);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_prompt_order_matches_cli_stdin_order() {
    let (root, mut app, projs) = tui_fixture("tui-order");
    // Default: confirm first, then the commit message.
    app.open_palette();
    type_line(&mut app, &format!("rm proj {}", projs[0].project_key));
    let labels = app.prompt_session.as_ref().unwrap().prompts.clone();
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(labels[0].contains("[y/N]") && labels[1].contains("Commit Message"), "{labels:?}");
    app.close_command_prompt();
    // --yes: only the commit message.
    app.open_palette();
    type_line(&mut app, &format!("rm proj {} --yes", projs[0].project_key));
    let labels = app.prompt_session.as_ref().unwrap().prompts.clone();
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert!(labels[0].contains("Commit Message"), "{labels:?}");
    app.close_command_prompt();
    // --yes --message: no prompts, runs immediately.
    app.open_palette();
    type_line(&mut app, &format!("rm proj {} --yes --message bye", projs[0].project_key));
    assert!(app.prompt_session.is_none());
    assert!(app.output.as_ref().unwrap().text.contains("Committed: bye"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_d_key_is_ignored_outside_project_view() {
    let (root, mut app, _) = tui_fixture("tui-ignore");
    app.enter_view(View::Status, "");
    term::handle_key(&mut app, &KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT));
    assert!(!app.palette_open);
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// Web API (router + the same handlers the server mounts)
// ---------------------------------------------------------------------------

#[test]
fn web_assets_have_delete_ui_and_script_is_embedded() {
    // The UI is embedded in the binary; check the source of truth.
    let html = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/static/index.html")).unwrap();
    for needle in [
        "id=\"deleteProjectBtn\"",
        "id=\"deleteProjectOverlay\"",
        "/delete'",
        "function openDeleteProject",
        "g d",
    ] {
        assert!(html.contains(needle), "missing {needle}");
    }
}

// ---------------------------------------------------------------------------
// Task delete (= archive) from the web UI, and `lun task unarchive`
// ---------------------------------------------------------------------------

#[test]
fn cli_task_unarchive_round_trip_and_errors() {
    let (root, cli, lun) = cli_fixture("cli-unarchive");
    let p = mk_proj(&lun, "home");
    let t = mk_task(&lun, "paint fence", Some(p.id));
    let run = |args: &[&str], input: &[&str]| {
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        run_result_in_reader(&cli, &argv, &mut reader(input), Some(&root))
    };

    // Not archived yet: says so, changes nothing, no commit prompt needed.
    let out = run(&["task", "unarchive", &t.task_key], &[]).unwrap();
    assert!(out.contains("is not archived"), "{out}");

    run(&["task", "archive", &t.task_key], &[""]).unwrap();
    assert!(lun.task_by_id(t.id).unwrap().archived_at.is_some());
    // Hidden from the default list, shown with --all.
    let hidden = run(&["task", "ls"], &[]).unwrap();
    assert!(!hidden.contains("paint fence"), "{hidden}");
    let all = run(&["task", "ls", "--all"], &[]).unwrap();
    assert!(all.contains("paint fence"), "{all}");

    let out = run(&["task", "unarchive", &t.task_key], &["bring it back"]).unwrap();
    assert!(out.contains("Unarchived") && out.contains("Committed: bring it back"), "{out}");
    assert!(lun.task_by_id(t.id).unwrap().archived_at.is_none());
    let log = &lun.logs_for("task", t.id).unwrap()[0];
    assert_eq!((log.action.as_str(), log.message.as_str()), ("UNARCHIVE", "bring it back"));
    assert!(run(&["task", "ls"], &[]).unwrap().contains("paint fence"));

    // --message is honoured (no stdin prompt), junk arguments are refused.
    run(&["task", "archive", &t.task_key], &[""]).unwrap();
    let out = run(&["task", "unarchive", &t.task_key, "--message", "via flag"], &[]).unwrap();
    assert!(out.contains("Committed: via flag"), "{out}");
    assert_eq!(run(&["task", "unarchive", &t.task_key, "bogus"], &[]).unwrap_err().kind(), "usage");

    // Errors.
    assert_eq!(run(&["task", "unarchive"], &[]).unwrap_err().kind(), "usage");
    assert_eq!(run(&["task", "unarchive", "T-nope"], &[]).unwrap_err().kind(), "not-found");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn archived_task_keeps_its_data_and_counts_for_project_delete() {
    let root = temp_root("archived-keeps");
    let lun = Lun::init(&root).unwrap();
    let p = mk_proj(&lun, "keepme");
    let t = mk_task(&lun, "keep my links", Some(p.id));
    lun.add_link(LinkTarget::Task(t.id), "doc", "https://x", None, None).unwrap();
    lun.update_task(t.id, TaskUpdateSpec { notes: Some("important".into()), ..Default::default() }).unwrap();
    lun.archive_task(t.id, None, Some("web")).unwrap();
    lun.unarchive_task(t.id, Some("web")).unwrap();
    let back = lun.task_by_id(t.id).unwrap();
    assert_eq!(back.notes, "important");
    assert_eq!(back.status, "planning");
    assert_eq!(lun.links_for_task(t.id).unwrap().len(), 1);
    // Archive twice / unarchive twice are harmless.
    lun.archive_task(t.id, None, None).unwrap();
    lun.archive_task(t.id, None, None).unwrap();
    lun.unarchive_task(t.id, None).unwrap();
    lun.unarchive_task(t.id, None).unwrap();
    let actions: Vec<String> = lun.logs_for("task", t.id).unwrap().into_iter().map(|l| l.action).collect();
    assert_eq!(actions.iter().filter(|a| *a == "ARCHIVE").count(), 2);
    assert_eq!(actions.iter().filter(|a| *a == "UNARCHIVE").count(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn web_assets_have_task_delete_archive_and_undo() {
    let html = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/static/index.html")).unwrap();
    for needle in [
        "id=\"deleteTaskBtn\"",
        "id=\"deleteTaskOverlay\"",
        "id=\"undoToast\"",
        "'/delete'",            // permanent delete from the button
        "'/archive'",           // archive from the x shortcut
        "'/unarchive'",
        "function archiveTask",
        "function confirmDeleteTask",
        "function undoDeleteTask",
    ] {
        assert!(html.contains(needle), "missing {needle}");
    }
    // x archives (it used to close the task).
    assert!(html.contains("archiveTask(activeCard.key"), "x should call archiveTask");
    assert!(!html.contains("markDone"), "markDone is gone");
}

// ---------------------------------------------------------------------------
// `lun rm task`: permanent delete
// ---------------------------------------------------------------------------

fn rm_task(cli: &CliApp, root: &std::path::Path, args: &[&str], input: &[&str]) -> lun::db::Result<String> {
    let mut argv: Vec<String> = vec!["rm".into(), "task".into()];
    argv.extend(args.iter().map(|s| s.to_string()));
    run_result_in_reader(cli, &argv, &mut reader(input), Some(root))
}

#[test]
fn db_delete_task_removes_every_trace_and_nothing_else() {
    let root = temp_root("db-deltask");
    let lun = Lun::init(&root).unwrap();
    let p = mk_proj(&lun, "home");
    let doomed = mk_task(&lun, "doomed", Some(p.id));
    let safe = mk_task(&lun, "safe", Some(p.id));
    for t in [&doomed, &safe] {
        lun.add_link(LinkTarget::Task(t.id), "l", "https://x", None, None).unwrap();
        lun.add_attachment_to(AttachmentTarget::Task(t.id), "f.txt", &format!("/tmp/nope-{}", t.task_key), None, None).unwrap();
        lun.update_task(t.id, TaskUpdateSpec { notes: Some("one".into()), ..Default::default() }).unwrap();
        lun.update_task(t.id, TaskUpdateSpec { notes: Some("two".into()), ..Default::default() }).unwrap();
    }
    lun.create_pr(PrSpec {
        task_id: doomed.id,
        source_branch: Some("feat".into()),
        target_branch: Some("main".into()),
        message: None,
        user: None,
    })
    .unwrap();
    let safe_logs = count(&root, &format!("SELECT COUNT(*) FROM logs WHERE entity_type='task' AND entity_id={}", safe.id));
    let safe_diffs = count(&root, &format!("SELECT COUNT(*) FROM note_diffs WHERE task_id={}", safe.id));
    assert!(safe_diffs >= 1);

    let done = lun.delete_task(doomed.id, Some("gone"), Some("t")).unwrap();
    assert_eq!(done.task.task_key, doomed.task_key);
    assert_eq!((done.links, done.attachments, done.prs), (1, 1, 1));
    assert!(done.log_entries >= 4, "{}", done.log_entries);
    assert_eq!(done.attachment_paths, vec![format!("/tmp/nope-{}", doomed.task_key)]);

    assert!(lun.task_by_id(doomed.id).is_err());
    assert!(lun.task_by_key(&doomed.task_key).is_err());
    for (table, col) in [("links", "task_id"), ("attachments", "task_id"), ("prs", "task_id"), ("note_diffs", "task_id")] {
        assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM {table} WHERE {col}={}", doomed.id)), 0, "{table}");
    }
    assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM logs WHERE entity_type='task' AND entity_id={}", doomed.id)), 0);
    // The other task is untouched, down to its history and diffs.
    assert!(lun.task_by_id(safe.id).is_ok());
    assert_eq!(lun.links_for_task(safe.id).unwrap().len(), 1);
    assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM logs WHERE entity_type='task' AND entity_id={}", safe.id)), safe_logs);
    assert_eq!(count(&root, &format!("SELECT COUNT(*) FROM note_diffs WHERE task_id={}", safe.id)), safe_diffs);
    // A trace is left on the project: its own history is gone.
    let last = &lun.logs_for("project", p.id).unwrap()[0];
    assert_eq!((last.action.as_str(), last.message.as_str()), ("DELETE", "gone"));
    assert!(last.details.contains(&doomed.task_key) && last.details.contains("doomed"), "{}", last.details);
    {
        let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
        let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
        assert_eq!(stmt.query_map([], |_| Ok(())).unwrap().count(), 0, "dangling foreign keys");
    }
    // Gone means gone: a second delete is a clean not-found.
    assert_eq!(lun.delete_task(doomed.id, None, None).unwrap_err().kind(), "not-found");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn db_delete_task_works_for_archived_and_unassigned_tasks() {
    let root = temp_root("db-deltask-edge");
    let lun = Lun::init(&root).unwrap();
    // Archived task.
    let p = mk_proj(&lun, "x");
    let a = mk_task(&lun, "was archived", Some(p.id));
    lun.archive_task(a.id, None, None).unwrap();
    lun.delete_task(a.id, None, None).unwrap();
    assert!(lun.task_by_id(a.id).is_err());
    // A task with no project: the trace goes on P-000.
    let n = mk_task(&lun, "orphan", None);
    let p0 = lun.unassigned_project_id().unwrap();
    lun.delete_task(n.id, None, None).unwrap();
    let last = &lun.logs_for("project", p0).unwrap()[0];
    assert_eq!(last.action, "DELETE");
    assert!(last.message.contains("orphan"), "{}", last.message);
    // Deleting the project afterwards still works (trace entry cascades with it).
    lun.delete_project(p.id, ProjectDeleteMode::RefuseIfNotEmpty, None, None).unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rm_task_by_key_title_confirm_and_flags() {
    let (root, cli, lun) = cli_fixture("cli-rmtask");
    let p = mk_proj(&lun, "proj");
    let a = mk_task(&lun, "write the intro", Some(p.id));
    let b = mk_task(&lun, "dup title", Some(p.id));
    let c = mk_task(&lun, "dup title", Some(p.id));
    let d = mk_task(&lun, "by title", Some(p.id));
    lun.add_link(LinkTarget::Task(a.id), "l", "https://x", None, None).unwrap();

    // Declined: empty, "n", closed stdin. Nothing changes.
    for input in [vec![""], vec!["n"], vec![]] {
        let e = rm_task(&cli, &root, &[&a.task_key], &input).unwrap_err();
        assert_eq!(e.kind(), "declined", "{input:?}");
        assert!(e.to_string().contains("task archive"), "{e}");
    }
    assert!(lun.task_by_id(a.id).is_ok());

    // y, then a blank commit message takes the default; summary lists what went.
    let out = rm_task(&cli, &root, &[&a.task_key], &["y", ""]).unwrap();
    assert!(out.contains(&format!("Deleted task {}", a.task_key)), "{out}");
    assert!(out.contains("Also removed:") && out.contains("1 links"), "{out}");
    assert!(out.contains("Committed: delete task"), "{out}");
    assert!(lun.task_by_id(a.id).is_err());

    // By exact title with --yes --message, no prompts at all.
    let out = rm_task(&cli, &root, &["by title", "--yes", "--message", "bye"], &[]).unwrap();
    assert!(out.contains("Committed: bye"), "{out}");
    assert!(lun.task_by_id(d.id).is_err());

    // Ambiguous title is refused and deletes neither.
    let e = rm_task(&cli, &root, &["dup title", "--yes"], &[]).unwrap_err();
    assert_eq!(e.kind(), "ambiguous");
    assert!(lun.task_by_id(b.id).is_ok() && lun.task_by_id(c.id).is_ok());
    // ...but the key picks one.
    rm_task(&cli, &root, &[&b.task_key, "-y"], &[""]).unwrap();
    assert!(lun.task_by_id(b.id).is_err() && lun.task_by_id(c.id).is_ok());

    // Errors.
    assert_eq!(rm_task(&cli, &root, &[], &[]).unwrap_err().kind(), "usage");
    assert_eq!(rm_task(&cli, &root, &["T-nope", "--yes"], &[]).unwrap_err().kind(), "not-found");
    assert_eq!(rm_task(&cli, &root, &["a", "b"], &[]).unwrap_err().kind(), "usage");
    assert_eq!(rm_task(&cli, &root, &[&c.task_key, "--bogus"], &[]).unwrap_err().kind(), "usage");
    assert_eq!(rm_task(&cli, &root, &[&c.task_key, "--message"], &[]).unwrap_err().kind(), "usage");
    // A project key is not a task.
    assert_eq!(rm_task(&cli, &root, &[&p.project_key, "--yes"], &[]).unwrap_err().kind(), "not-found");
    assert!(lun.task_by_id(c.id).is_ok());
    assert!(lun.project_by_key(&p.project_key).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rm_task_removes_managed_attachment_copies_only() {
    let (root, cli, lun) = cli_fixture("cli-rmtask-files");
    let p = mk_proj(&lun, "files");
    let t = mk_task(&lun, "has files", Some(p.id));
    let dir = root.join(".lun/attachments");
    std::fs::create_dir_all(&dir).unwrap();
    let managed = dir.join("copy.txt");
    let outside = root.join("by-path.txt");
    std::fs::write(&managed, "x").unwrap();
    std::fs::write(&outside, "y").unwrap();
    lun.add_attachment_to(AttachmentTarget::Task(t.id), "copy.txt", &managed.display().to_string(), None, None).unwrap();
    lun.add_attachment_to(AttachmentTarget::Task(t.id), "by-path.txt", &outside.display().to_string(), None, None).unwrap();
    rm_task(&cli, &root, &[&t.task_key, "--yes"], &[""]).unwrap();
    assert!(!managed.exists(), "managed copy removed");
    assert!(outside.exists(), "a path-linked file is never deleted");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_task_archive_is_not_rm() {
    // Archive keeps the row; rm removes it. The two must not be confused.
    let (root, cli, lun) = cli_fixture("cli-arch-vs-rm");
    let p = mk_proj(&lun, "p");
    let t = mk_task(&lun, "keep me", Some(p.id));
    let argv: Vec<String> = vec!["task".into(), "archive".into(), t.task_key.clone()];
    run_result_in_reader(&cli, &argv, &mut reader(&[""]), Some(&root)).unwrap();
    assert!(lun.task_by_id(t.id).unwrap().archived_at.is_some());
    // rm works on an archived task and really removes it.
    rm_task(&cli, &root, &[&t.task_key, "--yes"], &[""]).unwrap();
    assert!(lun.task_by_id(t.id).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_completion_knows_rm_task() {
    let (root, cli, lun) = cli_fixture("cli-complete-task");
    let p = mk_proj(&lun, "p");
    let t = mk_task(&lun, "completable title", Some(p.id));
    let c = |words: &[&str]| {
        let w: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        lun::cli::complete_output(Some(&cli), &w)
    };
    let subs = c(&["lun", "rm", ""]);
    assert!(subs.lines().any(|l| l == "proj") && subs.lines().any(|l| l == "task"), "{subs}");
    let tasks = c(&["lun", "rm", "task", ""]);
    assert!(tasks.lines().any(|l| l == t.task_key) && tasks.lines().any(|l| l == "completable title"), "{tasks}");
    let flags = c(&["lun", "rm", "task", "x", "--"]);
    assert!(flags.contains("--yes") && flags.contains("--message") && !flags.contains("--cascade"), "{flags}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tui_rm_task_prompts_in_cli_order_and_deletes() {
    let (root, mut app, projs) = tui_fixture("tui-rmtask");
    let task_key = app.data.tasks[0].task_key.clone();
    // Default: y/N, then commit message.
    app.open_palette();
    type_line(&mut app, &format!("rm task {task_key}"));
    let labels = app.prompt_session.as_ref().unwrap().prompts.clone();
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(labels[0].contains("Permanently delete task") && labels[0].contains("[y/N]"), "{labels:?}");
    assert!(labels[1].contains("Commit Message"), "{labels:?}");
    // A "no" ends it without asking for a commit message, and keeps the task.
    type_line(&mut app, "n");
    assert!(app.prompt_session.is_none());
    let out = app.output.as_ref().unwrap();
    assert!(out.is_error && out.text.contains("not deleted"), "{}", out.text);
    assert!(app.data.tasks.iter().any(|t| t.task_key == task_key));
    // y + message deletes it.
    app.close_command_prompt();
    app.open_palette();
    type_line(&mut app, &format!("rm task {task_key}"));
    type_line(&mut app, "y");
    type_line(&mut app, "tui removal");
    let out = app.output.as_ref().unwrap();
    assert!(!out.is_error && out.text.contains("Deleted task") && out.text.contains("Committed: tui removal"), "{}", out.text);
    assert!(!app.data.tasks.iter().any(|t| t.task_key == task_key));
    assert!(app.task_selected <= app.data.tasks.len());
    // Unknown task is an error up front, no prompt.
    app.close_command_prompt();
    app.open_palette();
    type_line(&mut app, "rm task T-nope");
    assert!(app.prompt_session.is_none());
    let _ = projs;
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn binary_rm_task_exit_codes() {
    let home = std::fs::canonicalize(temp_root("bin-rmtask")).unwrap();
    let root = home.join("board");
    std::fs::create_dir_all(&root).unwrap();
    let bin = env!("CARGO_BIN_EXE_lun");
    let run = |args: &[&str]| {
        let out = Command::new(bin).args(args).current_dir(&root).env("HOME", &home)
            .stdin(std::process::Stdio::null()).output().unwrap();
        (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string())
    };
    assert_eq!(run(&["init"]).0, 0);
    let (_, out, _) = run(&["add", "task", "to remove", "--message", "m"]);
    let key = out.split("Created task ").nth(1).unwrap().split_whitespace().next().unwrap().to_string();
    // Closed stdin and no --yes: declined, exit 2, still there.
    let (code, _, err) = run(&["rm", "task", &key]);
    assert_eq!(code, 2, "{err}");
    assert_eq!(run(&["task", &key]).0, 0);
    // --yes: gone, exit 0.
    let (code, out, err) = run(&["rm", "task", &key, "--yes", "--message", "bin"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains(&format!("Deleted task {key}")), "{out}");
    assert_eq!(run(&["task", &key]).0, 2);
    // Already gone: not-found, exit 2.
    assert_eq!(run(&["rm", "task", &key, "--yes"]).0, 2);
    let (_, help, _) = run(&["--help"]);
    assert!(help.contains("lun rm task"), "{help}");
    let _ = std::fs::remove_dir_all(&home);
}
