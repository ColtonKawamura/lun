//! Phase 3 tests: the plan's example flows for `lun status`,
//! `lun proj add task`, `lun task`, and `lun log`, plus resolution rules
//! (exact match, ambiguous/unknown errors) and log-on-write verification.
//!
//! Fixture note: the plan's `lun status` example table and its per-project
//! example disagree with each other (9 vs 10 tasks listed for paper-stack);
//! the per-project example is authoritative, so the fixture mirrors it.

use std::io::{BufReader, Cursor};
use std::path::PathBuf;

use lun::cli::{
    create_task, grep_view, log_view, resolve_entity, resolve_project, resolve_task,
    run_result_in_reader, status_all, status_project, status_project_board, status_target,
    task_archive, task_complete, task_edit, task_list, task_reopen, task_view, App, EXIT_USAGE,
};
use lun::{Lun, ProjectSpec, TaskSpec};

/// Split a rendered table row on runs of 3+ spaces (columns are padded with
/// a 3-space gap, so any 3+-space run is a separator; cell content never
/// contains 3+ consecutive spaces).
fn split_cols(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            let mut j = i;
            while j < bytes.len() && bytes[j] == b' ' {
                j += 1;
            }
            if j - i >= 3 {
                out.push(line[start..i].to_string());
                start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out.push(line[start..].to_string());
    out
}

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lun-p3-test-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Build the plan's fixture: three real projects + P-000, and the plan's
/// example tasks in creation order (paper-stack 10, granE-friction 2,
/// lun-cli 2, Unassigned 2 = 16 tasks, T-001..T-016).
fn fixture() -> (PathBuf, Lun) {
    let root = temp_root("fixture");
    let lun = Lun::init(&root).unwrap();

    let ps = lun
        .create_project(ProjectSpec {
            name: "paper-stack".into(),
            status: Some("review".into()),
            ..Default::default()
        })
        .unwrap();
    let ge = lun
        .create_project(ProjectSpec {
            name: "granE-friction".into(),
            status: Some("active".into()),
            ..Default::default()
        })
        .unwrap();
    let lc = lun
        .create_project(ProjectSpec {
            name: "lun-cli".into(),
            ..Default::default()
        })
        .unwrap();

    let t = |lun: &Lun,
             project: Option<i64>,
             title: &str,
             status: &str,
             priority: &str,
             branch: Option<&str>| {
        lun.create_task(TaskSpec {
            title: title.into(),
            project,
            status: Some(status.into()),
            priority: Some(priority.into()),
            assignee: Some("me".into()),
            branch: branch.map(String::from),
            ..Default::default()
        })
        .unwrap()
    };

    // T-001..T-010: paper-stack (plan's per-project example order)
    let t010 = t(
        &lun,
        Some(ps.id),
        "Tune ball–chain damping params",
        "review",
        "high",
        Some("feat/damping-sweep"),
    );
    let t011 = t(
        &lun,
        Some(ps.id),
        "Analyze restitution vs stack size",
        "planning",
        "med",
        None,
    );
    let t012 = t(
        &lun,
        Some(ps.id),
        "Write methods section draft",
        "waiting-on",
        "high",
        Some("feat/methods-draft"),
    );
    t(
        &lun,
        Some(ps.id),
        "Set up ball–chain simulation",
        "closed",
        "high",
        Some("feat/chain-sim"),
    );
    t(
        &lun,
        Some(ps.id),
        "Implement restitution measurement",
        "closed",
        "high",
        Some("feat/restitution"),
    );
    t(
        &lun,
        Some(ps.id),
        "Explore stack length sweep",
        "closed",
        "med",
        None,
    );
    t(
        &lun,
        Some(ps.id),
        "Document dimensionless parameters",
        "closed",
        "med",
        None,
    );
    t(
        &lun,
        Some(ps.id),
        "Validate negligible-gravity regime",
        "closed",
        "med",
        None,
    );
    t(
        &lun,
        Some(ps.id),
        "Prepare figures for restitution plot",
        "closed",
        "med",
        None,
    );
    t(
        &lun,
        Some(ps.id),
        "Draft introduction section",
        "closed",
        "low",
        None,
    );

    // T-011..: granE-friction (2 planning)
    t(
        &lun,
        Some(ge.id),
        "Design frictional pack.m pipeline",
        "planning",
        "high",
        None,
    );
    t(
        &lun,
        Some(ge.id),
        "Save contact histories in pack.m",
        "planning",
        "med",
        None,
    );

    // lun-cli: 1 review, 1 closed
    t(
        &lun,
        Some(lc.id),
        "Implement `lun status` command",
        "review",
        "high",
        Some("feat/lun-status"),
    );
    t(
        &lun,
        Some(lc.id),
        "Add markdown board view",
        "closed",
        "low",
        Some("feat/board-view"),
    );

    // Unassigned: 2 planning
    t(
        &lun,
        None,
        "Sketch ideas for `lun board`",
        "planning",
        "med",
        None,
    );
    t(
        &lun,
        None,
        "Refactor personal dotfiles",
        "planning",
        "low",
        None,
    );

    let _ = (t010, t011, t012);
    (root, lun)
}

// ---------------------------------------------------------------------------
// lun status
// ---------------------------------------------------------------------------

#[test]
fn status_all_matches_plan_format() {
    let (_root, lun) = fixture();
    // Resolve dynamic project keys before `lun` moves into `app`.
    let p_paper = resolve_project(&lun, "paper-stack").unwrap().project_key;
    let p_gran = resolve_project(&lun, "granE-friction").unwrap().project_key;
    let p_cli = resolve_project(&lun, "lun-cli").unwrap().project_key;
    let app = App { lun };
    let out = status_all(&app).unwrap();

    // Section headers
    assert!(
        out.starts_with("Projects\n--------\n\n"),
        "Projects header: {out}"
    );
    // Project rows: key, name, status, planning/review/waiting-on/backlog/closed
    // (columns are padded; split on runs of 3+ spaces and compare cells)
    let proj_rows: Vec<Vec<String>> = out
        .lines()
        .filter(|l| l.starts_with("P-"))
        .map(split_cols)
        .collect();
    assert_eq!(proj_rows.len(), 4, "four project rows: {out}");
    let row = |key: &str| proj_rows.iter().find(|r| r[0] == key).unwrap();
    assert_eq!(
        row("P-000"),
        &["P-000", "Unassigned", "active", "2", "0", "0", "0", "0"]
    );
    assert_eq!(
        row(&p_paper),
        &[&p_paper, "paper-stack", "active", "1", "1", "1", "0", "7"]
    );
    assert_eq!(
        row(&p_gran),
        &[&p_gran, "granE-friction", "active", "2", "0", "0", "0", "0"]
    );
    assert_eq!(
        row(&p_cli),
        &[&p_cli, "lun-cli", "active", "0", "1", "0", "0", "1"]
    );

    // Summary line: 4 projects, 16 tasks (5 planning, 2 review, 1 waiting-on, 0 backlog, 8 closed)
    assert!(
        out.contains(
            "Summary: 4 projects · 16 tasks (5 planning, 2 review, 1 waiting-on, 0 backlog, 8 closed)"
        ),
        "summary line: {out}"
    );
}

#[test]
fn status_project_matches_plan_format() {
    let (_root, lun) = fixture();
    let p_key = resolve_project(&lun, "paper-stack").unwrap().project_key;
    let app = App { lun };
    let out = status_project(&app, "paper-stack").unwrap();

    // Header: name + underline of '='
    assert!(
        out.starts_with("Project: paper-stack\n====================\n"),
        "header: {out}"
    );

    // Overview block
    assert!(
        out.contains(
            &format!("Overview\n--------\n\nID:      {p_key}\nName:    paper-stack\nStatus:  active\n")
        ),
        "overview: {out}"
    );

    // Tasks by Status block (plan format: "- <status>:<padding><n>", 14-wide)
    assert!(
        out.contains("Tasks by Status:\n- planning:     1\n- review:       1\n- waiting-on:   1\n- backlog:      0\n- closed:       7\n"),
        "counts by status: {out}"
    );

    // Task rows with project column (cells parsed, widths are dynamic)
    let t_rows: Vec<Vec<String>> = out
        .lines()
        .filter(|l| l.starts_with("T-"))
        .map(split_cols)
        .collect();
    assert_eq!(t_rows.len(), 10, "ten task rows: {out}");
    // Resolve dynamic task key for the first task
    let task1 = resolve_task(&app.lun, "Tune ball–chain damping params").unwrap();
    assert_eq!(
        t_rows[0],
        &[
            &task1.task_key,
            "paper-stack",
            "Tune ball–chain damping params",
            "review",
            "high",
            "me",
            "feat/damping-sweep"
        ]
    );

    // Summary: 1 project · 10 tasks
    assert!(
        out.contains(
            "Summary: 1 project · 10 tasks (1 planning, 1 review, 1 waiting-on, 0 backlog, 7 closed)"
        ),
        "summary: {out}"
    );
}

#[test]
fn status_project_board_groups_by_all_statuses() {
    let (_root, lun) = fixture();
    // Resolve dynamic task keys before `lun` moves into `app`.
    let task_analyze = resolve_task(&lun, "Analyze restitution vs stack size").unwrap();
    let task_tune = resolve_task(&lun, "Tune ball–chain damping params").unwrap();
    let task_write = resolve_task(&lun, "Write methods section draft").unwrap();
    let app = App { lun };
    let out = status_project_board(&app, "paper-stack").unwrap();
    assert!(out.contains("Board: paper-stack"));
    assert!(out.contains(&format!("Planning\n--------\n- {}  Analyze restitution vs stack size", task_analyze.task_key)));
    assert!(out.contains(&format!("Review\n------\n- {}  Tune ball–chain damping params", task_tune.task_key)));
    assert!(out.contains(&format!("Waiting-On\n----------\n- {}  Write methods section draft", task_write.task_key)));
    assert!(out.contains("Backlog\n-------\n- (none)"));
}

#[test]
fn status_by_key_and_name_are_identical() {
    let (_root, lun) = fixture();
    let proj_key = resolve_project(&lun, "paper-stack").unwrap().project_key;
    let app = App { lun };
    let by_name = status_project(&app, "paper-stack").unwrap();
    let by_key = status_project(&app, &proj_key).unwrap();
    assert_eq!(
        by_name, by_key,
        "`lun status paper-stack` and `lun status <project key>` must be identical"
    );
}

#[test]
fn status_unknown_project_errors() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let e = status_project(&app, "no-such-project").unwrap_err();
    assert_eq!(e.kind(), "not-found");
    assert!(e.to_string().contains("no-such-project"));
    let e = status_project(&app, "P-999").unwrap_err();
    assert_eq!(e.kind(), "not-found");
}

// ---------------------------------------------------------------------------
// lun proj add task
// ---------------------------------------------------------------------------

/// Buffered reader over canned prompt answers (one line each).
fn prompt_reader(lines: &[&str]) -> std::io::BufReader<std::io::Cursor<Vec<u8>>> {
    let mut buf: Vec<u8> = Vec::new();
    for l in lines {
        buf.extend_from_slice(l.as_bytes());
        buf.push(b'\n');
    }
    std::io::BufReader::new(std::io::Cursor::new(buf))
}

#[test]
fn create_task_flow_with_explicit_answers() {
    let root = temp_root("explicit");
    let lun = Lun::init(&root).unwrap();
    lun.create_project(ProjectSpec {
        name: "paper-stack".into(),
        ..Default::default()
    })
    .unwrap();
    let app = App { lun };

    // Next key: one past the existing 0 tasks -> T-001.
    let mut input = prompt_reader(&["planning", "med", "me", "", ""]);
    let out = create_task(&app, "my task title", Some("paper-stack"), &mut input).unwrap();
    let lines = out.split('\n').collect::<Vec<_>>();
    // Resolve the created task's key
    let created_task = resolve_task(&app.lun, "my task title").unwrap();
    assert_eq!(lines[0], &format!("Created task {} in project paper-stack", created_task.task_key));
    assert_eq!(
        lines[1],
        "Committed: add task \"my task title\" to paper-stack"
    );

    // Task exists in DB with the answered fields
    let task = app.lun.task_by_key(&created_task.task_key).unwrap();
    assert_eq!(task.title, "my task title");
    assert_eq!(task.status, "planning");
    assert_eq!(task.priority, "med");
    assert_eq!(task.assignee.as_deref(), Some("me"));

    // Log-on-write: a CREATE entry with the commit message and details.
    let entries = app.lun.logs_for("task", task.id).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, "CREATE");
    assert_eq!(
        entries[0].message,
        "add task \"my task title\" to paper-stack"
    );
    assert!(entries[0].details.contains("\"status\": \"planning\""));
    assert!(entries[0].details.contains("\"priority\": \"med\""));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn create_task_uses_defaults_for_empty_input() {
    let root = temp_root("defaults");
    let lun = Lun::init(&root).unwrap();
    // No user projects: default project is Unassigned (P-000).
    let app = App { lun };
    let mut input = prompt_reader(&["", "", "", "", ""]);
    let out = create_task(&app, "bare task", None, &mut input).unwrap();
    let lines = out.split('\n').collect::<Vec<_>>();
    // Resolve created task key
    let created = resolve_task(&app.lun, "bare task").unwrap();
    assert_eq!(lines[0], &format!("Created task {} in project Unassigned", created.task_key));
    assert_eq!(lines[1], "Committed: add task \"bare task\" to Unassigned");

    let t = app.lun.task_by_key(&created.task_key).unwrap();
    assert_eq!(t.status, "planning");
    assert_eq!(t.priority, "low");
    assert_eq!(t.assignee.as_deref(), Some("me"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn create_task_with_explicit_project_query() {
    let root = temp_root("single-proj");
    let lun = Lun::init(&root).unwrap();
    lun.create_project(ProjectSpec {
        name: "solo".into(),
        ..Default::default()
    })
    .unwrap();
    let app = App { lun };
    let mut input = prompt_reader(&["review", "high", "me", "custom commit message"]);
    let out = create_task(&app, "solo task", Some("solo"), &mut input).unwrap();
    let key = resolve_task(&app.lun, "solo task").unwrap().task_key;
    assert_eq!(
        out.lines().next().unwrap(),
        &format!("Created task {} in project solo", key)
    );
    assert_eq!(
        out.lines().nth(1).unwrap(),
        "Committed: custom commit message"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn create_task_defaults_project_to_unassigned_when_no_projects() {
    // No user projects exist: the task lands in the implicit P-000 bucket.
    let root = temp_root("no-proj");
    let lun = Lun::init(&root).unwrap();
    let app = App { lun };
    let mut input = prompt_reader(&["planning", "med", "", ""]);
    let out = create_task(&app, "unhome task", None, &mut input).unwrap();
    let task_key = resolve_task(&app.lun, "unhome task").unwrap().task_key;
    assert_eq!(
        out.lines().next().unwrap(),
        &format!("Created task {} in project Unassigned", task_key)
    );
    assert_eq!(
        out.lines().nth(1).unwrap(),
        "Committed: add task \"unhome task\" to Unassigned"
    );
    let t = app.lun.task_by_key(&task_key).unwrap();
    let p000 = app.lun.project_by_key("P-000").unwrap();
    assert_eq!(t.project_id, Some(p000.id), "task belongs to the P-000 row");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn create_task_rejects_invalid_status_and_priority() {
    let root = temp_root("invalid-vals");
    let lun = Lun::init(&root).unwrap();
    let app = App { lun };
    let mut input = prompt_reader(&["bogus", "", "", "", ""]);
    let e = create_task(&app, "bad status", None, &mut input).unwrap_err();
    assert_eq!(e.kind(), "invalid");
    assert!(e.to_string().contains("invalid status 'bogus'"));

    let mut input = prompt_reader(&["planning", "urgent", "", "", ""]);
    let e = create_task(&app, "bad priority", None, &mut input).unwrap_err();
    assert_eq!(e.kind(), "invalid");
    assert!(e.to_string().contains("invalid priority 'urgent'"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn create_task_rejects_empty_title() {
    let root = temp_root("empty-title");
    let lun = Lun::init(&root).unwrap();
    let app = App { lun };
    let mut input = prompt_reader(&["planning"]);
    let e = create_task(&app, "   ", None, &mut input).unwrap_err();
    assert_eq!(e.kind(), "usage");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// lun task
// ---------------------------------------------------------------------------

#[test]
fn task_view_matches_plan_format() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let out = task_view(&app, "Write methods section draft").unwrap(); // title

    // Resolve task key
    let task = resolve_task(&app.lun, "Write methods section draft").unwrap();
    // Header
    assert!(out.starts_with(&format!("Task {}\n", task.task_key)), "header: {out}");
    assert!(
        out.lines().nth(1).unwrap().chars().all(|c| c == '='),
        "underline row: {out}"
    );

    // Fields
    assert!(
        out.contains("Project:   paper-stack\n"),
        "project field: {out}"
    );
    assert!(
        out.contains("Title:     Write methods section draft\n"),
        "title field: {out}"
    );
    assert!(
        out.contains("Status:    waiting-on\n"),
        "status field: {out}"
    );
    assert!(out.contains("Priority:  high\n"), "priority field: {out}");
    assert!(out.contains("Assignee:  me\n"), "assignee field: {out}");
    assert!(out.contains("Labels:    []\n"), "labels field: {out}");
    assert!(
        out.contains("Branch:    feat/methods-draft\n"),
        "branch field: {out}"
    );
    assert!(
        out.contains("Created:   ") && out.contains("Updated:   "),
        "timestamps: {out}"
    );

    assert!(
        out.contains(&format!("Description:\n- (add a description with `lun task edit {} --notes \"...\"`)", task.task_key)),
        "description stub: {out}"
    );

    // History (log): the CREATE entry with compact detail + commit line.
    assert!(out.contains("History (log):\n"), "history section: {out}");
    assert!(
        out.contains("  me  CREATE\n    Status: waiting-on, Priority: high\n    Commit: add task \"Write methods section draft\" to paper-stack"),
        "CREATE history entry: {out}"
    );
}

#[test]
fn task_view_by_key() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let task = resolve_task(&app.lun, "Tune ball–chain damping params").unwrap();
    let out = task_view(&app, &task.task_key).unwrap();
    assert!(out.starts_with(&format!("Task {}\n", task.task_key)), "header: {out}");
    assert!(
        out.contains("Title:     Tune ball–chain damping params"),
        "title: {out}"
    );
}

#[test]
fn task_view_unknown_and_ambiguous() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let e = task_view(&app, "T-999").unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let e = task_view(&app, "no such title here").unwrap_err();
    assert_eq!(e.kind(), "not-found");
}

#[test]
fn status_for_task_shows_task_data_with_last_commit_only() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let task = resolve_task(&app.lun, "Write methods section draft").unwrap();
    let out = status_target(&app, "Write methods section draft").unwrap();
    assert!(out.starts_with(&format!("**Task {}**", task.task_key)), "header: {out}");
    assert!(out.contains("Project:   paper-stack"), "project: {out}");
    assert!(
        out.contains("Title:     Write methods section draft"),
        "title: {out}"
    );
    assert!(out.contains("Status:    waiting-on"), "status: {out}");
    assert!(out.contains("Priority:  high"), "priority: {out}");
    assert!(out.contains("**Description:**"), "description: {out}");
    assert!(
        out.contains("**Last Commit:**"),
        "last commit section: {out}"
    );
    assert!(
        out.contains("  me  CREATE\n    Status: waiting-on, Priority: high\n    Commit: add task"),
        "last commit entry: {out}"
    );
    assert!(
        !out.contains("History (log):"),
        "should not show full history: {out}"
    );
}

#[test]
fn task_list_edit_complete_reopen_and_archive_work() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let task = resolve_task(&app.lun, "Write methods section draft").unwrap();
    let listed = task_list(
        &app,
        &[
            "--project".into(),
            "paper-stack".into(),
            "--status".into(),
            "waiting-on".into(),
            "--sort".into(),
            "title".into(),
        ],
    )
    .unwrap();
    assert!(listed.contains(&task.task_key));
    assert!(!listed.contains("T-001"));

    let mut input = prompt_reader(&[""]);
    let edited = task_edit(
        &app,
        &task.task_key,
        &[
            "--status".into(),
            "review".into(),
            "--priority".into(),
            "med".into(),
            "--branch".into(),
            "feat/edited".into(),
            "--labels".into(),
            "docs,phase11".into(),
        ],
        &mut input,
    )
    .unwrap();
    assert!(edited.contains(&format!("Updated {}", task.task_key)));

    let out = task_view(&app, &task.task_key).unwrap();
    assert!(out.contains("Status:    review"));
    assert!(out.contains("Priority:  med"));
    assert!(out.contains("Branch:    feat/edited"));
    assert!(out.contains("Labels:    [\"docs\", \"phase11\"]"));

    let mut input = prompt_reader(&[""]);
    let completed = task_complete(&app, &task.task_key, &mut input).unwrap();
    assert!(completed.contains(&format!("Completed {}", task.task_key)));
    assert!(task_view(&app, &task.task_key)
        .unwrap()
        .contains("Status:    closed"));

    let mut input = prompt_reader(&[""]);
    let reopened = task_reopen(&app, &task.task_key, &[], &mut input).unwrap();
    assert!(reopened.contains(&format!("Reopened {}", task.task_key)));
    assert!(task_view(&app, &task.task_key)
        .unwrap()
        .contains("Status:    planning"));

    let mut input = prompt_reader(&[""]);
    let archived = task_archive(&app, &task.task_key, &mut input).unwrap();
    assert!(archived.contains(&format!("Archived {}", task.task_key)));
    let archived_view = task_view(&app, &task.task_key).unwrap();
    assert!(archived_view.contains("Archived:  yes"));
    assert!(!task_list(&app, &[]).unwrap().contains(&task.task_key));
    assert!(task_list(&app, &["--all".into()])
        .unwrap()
        .contains(&task.task_key));
}

// ---------------------------------------------------------------------------
// lun log
// ---------------------------------------------------------------------------

#[test]
fn log_for_task_matches_plan_format() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let out = log_view(&app, "Write methods section draft", false).unwrap();

    // Compact commit-style entry with bullet marker.
    assert!(
        out.contains("- ") && out.contains("  me  CREATE\n    Status: waiting-on, Priority: high"),
        "CREATE entry: {out}"
    );
}

#[test]
fn log_for_task_by_key() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let task = resolve_task(&app.lun, "Write methods section draft").unwrap();
    let out = log_view(&app, &task.task_key, false).unwrap();
    assert!(out.contains("  me  CREATE"), "entry present: {out}");
}

#[test]
fn log_for_project_lists_task_history_newest_first() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let out = log_view(&app, "paper-stack", false).unwrap();
    // Newest first: the last-created paper-stack task must appear before the first.
    let last = out
        .find("Draft introduction section")
        .expect("last task in log");
    let first = out
        .find("Set up ball–chain simulation")
        .expect("first task in log");
    assert!(last < first, "newest-first ordering: {out}");

    // Each entry carries the action + compact detail line.
    assert!(out.contains("me  CREATE"), "action-style lines: {out}");
    assert!(
        out.contains("Status: closed, Priority: med"),
        "detail line: {out}"
    );
    assert!(
        out.contains("Commit: add task \"Write methods section draft\" to paper-stack"),
        "commit line: {out}"
    );
}

#[test]
fn log_resolves_project_before_task_on_name_collision() {
    let root = temp_root("collision");
    let lun = Lun::init(&root).unwrap();
    let p = lun
        .create_project(ProjectSpec {
            name: "dup".into(),
            ..Default::default()
        })
        .unwrap();
    lun.create_task(TaskSpec {
        title: "dup".into(),
        project: Some(p.id),
        ..Default::default()
    })
    .unwrap();

    let e = resolve_entity(&lun, "dup").unwrap();
    match e {
        lun::cli::Entity::Project(pr) => assert_eq!(pr.name, "dup"),
        other => panic!("expected project, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn log_unknown_entity_errors() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let e = log_view(&app, "ghost", false).unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let e = log_view(&app, "T-999", false).unwrap_err();
    assert_eq!(e.kind(), "not-found");
}

// ---------------------------------------------------------------------------
// lun grep
// ---------------------------------------------------------------------------

#[test]
fn grep_searches_projects_tasks_and_commits() {
    let (_root, lun) = fixture();
    let p_key = lun.project_by_name("paper-stack").unwrap().project_key;
    let app = App { lun };
    let out = grep_view(&app, "paper-stack").unwrap();
    let task1 = resolve_task(&app.lun, "Tune ball–chain damping params").unwrap();
    let task3 = resolve_task(&app.lun, "Write methods section draft").unwrap();
    assert!(out.starts_with("Grep: paper-stack\n"), "header: {out}");
    assert!(out.contains("Projects\n--------"), "project section: {out}");
    assert!(
        out.contains(&p_key) && out.contains("Name") && out.contains("paper-stack"),
        "project hit: {out}"
    );
    assert!(out.contains("Tasks\n-----"), "task section: {out}");
    assert!(
        out.contains(&task1.task_key) && out.contains("Project   paper-stack"),
        "task hit: {out}"
    );
    assert!(out.contains("Commits\n-------"), "commit section: {out}");
    assert!(
        out.contains(&task3.task_key) && out.contains("Commit") && out.contains("paper-stack"),
        "commit hit: {out}"
    );
    assert!(out.contains("Summary:"), "summary: {out}");
}

#[test]
fn grep_requires_non_empty_query() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let e = grep_view(&app, "   ").unwrap_err();
    assert_eq!(e.kind(), "usage");
}

// ---------------------------------------------------------------------------
// Resolution unit checks
// ---------------------------------------------------------------------------

#[test]
fn resolution_is_exact() {
    let (_root, lun) = fixture();
    // No substring matching: "paper" is not "paper-stack".
    assert_eq!(
        resolve_project(&lun, "paper").unwrap_err().kind(),
        "not-found"
    );
    assert_eq!(
        resolve_task(&lun, "damping").unwrap_err().kind(),
        "not-found"
    );
    // Exact title match works.
    let t = resolve_task(&lun, "Write methods section draft").unwrap();
    assert_eq!(t.task_key, lun.task_by_id(3).unwrap().task_key);
    assert!(t.task_key.starts_with("T-"));
}

#[test]
fn ambiguous_task_title_errors() {
    let root = temp_root("ambiguous");
    let lun = Lun::init(&root).unwrap();
    for _ in 0..2 {
        lun.create_task(TaskSpec {
            title: "same title".into(),
            ..Default::default()
        })
        .unwrap();
    }
    let t1_key = lun.task_by_id(1).unwrap().task_key;
    let t2_key = lun.task_by_id(2).unwrap().task_key;
    let e = resolve_task(&lun, "same title").unwrap_err();
    assert_eq!(e.kind(), "ambiguous");
    assert!(e.to_string().contains(&format!("{t1_key}, {t2_key}")));
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// Dispatch + exit codes (run() with a non-interactive command)
// ---------------------------------------------------------------------------

#[test]
fn run_returns_usage_exit_for_unknown_commands() {
    let root = temp_root("dispatch");
    {
        let lun = Lun::init(&root).unwrap();
        drop(lun);
    }
    let app = App::open(&root).unwrap();
    let usage = std::process::ExitCode::from(EXIT_USAGE);
    let args: Vec<String> = vec!["frobnicate".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    let args: Vec<String> = vec!["task".into()]; // missing argument
    assert_eq!(lun::cli::run(&app, &args), usage);
    let args: Vec<String> = vec!["proj".into(), "frobnicate".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_status_prints_successfully() {
    let root = temp_root("dispatch-status");
    let lun = Lun::init(&root).unwrap();
    let app = App { lun };
    let args: Vec<String> = vec!["status".into()];
    assert_eq!(lun::cli::run(&app, &args), std::process::ExitCode::SUCCESS);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn slash_prefixed_query_runs_status_in_cli() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let mut stdin = BufReader::new(Cursor::new(Vec::<u8>::new()));
    let out = run_result_in_reader(&app, &["/paper-stack".to_string()], &mut stdin, None).unwrap();
    let expected = status_target(&app, "paper-stack").unwrap();
    assert_eq!(out, expected);
}
