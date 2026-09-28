//! Phase 3 tests: the plan's example flows for `lun status`,
//! `lun proj add task`, `lun task`, and `lun log`, plus resolution rules
//! (exact match, ambiguous/unknown errors) and log-on-write verification.
//!
//! Fixture note: the plan's `lun status` example table and its per-project
//! example disagree with each other (9 vs 10 tasks listed for paper-stack);
//! the per-project example is authoritative, so the fixture mirrors it.

use std::path::PathBuf;

use lun::cli::{
    create_task, log_view, resolve_entity, resolve_project, resolve_task, status_all,
    status_project, status_project_board, status_target, task_archive, task_complete, task_edit, task_list,
    task_reopen, task_view, App, EXIT_USAGE,
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
            status: Some("doing".into()),
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
        "doing",
        "high",
        Some("feat/damping-sweep"),
    );
    let t011 = t(
        &lun,
        Some(ps.id),
        "Analyze restitution vs stack size",
        "todo",
        "med",
        None,
    );
    let t012 = t(
        &lun,
        Some(ps.id),
        "Write methods section draft",
        "follow-up",
        "high",
        Some("feat/methods-draft"),
    );
    t(
        &lun,
        Some(ps.id),
        "Set up ball–chain simulation",
        "done",
        "high",
        Some("feat/chain-sim"),
    );
    t(
        &lun,
        Some(ps.id),
        "Implement restitution measurement",
        "done",
        "high",
        Some("feat/restitution"),
    );
    t(
        &lun,
        Some(ps.id),
        "Explore stack length sweep",
        "done",
        "med",
        None,
    );
    t(
        &lun,
        Some(ps.id),
        "Document dimensionless parameters",
        "done",
        "med",
        None,
    );
    t(
        &lun,
        Some(ps.id),
        "Validate negligible-gravity regime",
        "done",
        "med",
        None,
    );
    t(
        &lun,
        Some(ps.id),
        "Prepare figures for restitution plot",
        "done",
        "med",
        None,
    );
    t(
        &lun,
        Some(ps.id),
        "Draft introduction section",
        "done",
        "low",
        None,
    );

    // T-011..: granE-friction (2 todo)
    t(
        &lun,
        Some(ge.id),
        "Design frictional pack.m pipeline",
        "todo",
        "high",
        None,
    );
    t(
        &lun,
        Some(ge.id),
        "Save contact histories in pack.m",
        "todo",
        "med",
        None,
    );

    // lun-cli: 1 doing, 1 done
    t(
        &lun,
        Some(lc.id),
        "Implement `lun status` command",
        "doing",
        "high",
        Some("feat/lun-status"),
    );
    t(
        &lun,
        Some(lc.id),
        "Add markdown board view",
        "done",
        "low",
        Some("feat/board-view"),
    );

    // Unassigned: 2 todo
    t(
        &lun,
        None,
        "Sketch ideas for `lun board`",
        "todo",
        "med",
        None,
    );
    t(
        &lun,
        None,
        "Refactor personal dotfiles",
        "todo",
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
    let app = App { lun };
    let out = status_all(&app).unwrap();

    // Section headers
    assert!(
        out.starts_with("Projects\n--------\n\n"),
        "Projects header: {out}"
    );
    // Project rows: key, name, status, todo/doing/follow-up/blocked/done
    // (columns are padded; split on runs of 3+ spaces and compare cells)
    let proj_rows: Vec<Vec<String>> = out
        .lines()
        .filter(|l| l.starts_with("P-00"))
        .map(split_cols)
        .collect();
    assert_eq!(proj_rows.len(), 4, "four project rows: {out}");
    let row = |key: &str| proj_rows.iter().find(|r| r[0] == key).unwrap();
    assert_eq!(
        row("P-000"),
        &["P-000", "Unassigned", "active", "2", "0", "0", "0", "0"]
    );
    assert_eq!(
        row("P-001"),
        &["P-001", "paper-stack", "active", "1", "1", "1", "0", "7"]
    );
    assert_eq!(
        row("P-002"),
        &["P-002", "granE-friction", "active", "2", "0", "0", "0", "0"]
    );
    assert_eq!(
        row("P-003"),
        &["P-003", "lun-cli", "active", "0", "1", "0", "0", "1"]
    );

    // Summary line: 4 projects, 16 tasks (5 todo, 2 doing, 1 follow-up, 0 blocked, 8 done)
    assert!(
        out.contains(
            "Summary: 4 projects · 16 tasks (5 todo, 2 doing, 1 follow-up, 0 blocked, 8 done)"
        ),
        "summary line: {out}"
    );
}

#[test]
fn status_project_matches_plan_format() {
    let (_root, lun) = fixture();
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
            "Overview\n--------\n\nID:      P-001\nName:    paper-stack\nStatus:  active\n"
        ),
        "overview: {out}"
    );

    // Tasks by Status block (plan format: "- <status>:<padding><n>", 14-wide)
    assert!(
        out.contains("Tasks by Status:\n- todo:         1\n- doing:        1\n- follow-up:    1\n- blocked:      0\n- done:         7\n"),
        "counts by status: {out}"
    );

    // Task rows with project column (cells parsed, widths are dynamic)
    let t_rows: Vec<Vec<String>> = out
        .lines()
        .filter(|l| l.starts_with("T-0"))
        .map(split_cols)
        .collect();
    assert_eq!(t_rows.len(), 10, "ten task rows: {out}");
    assert_eq!(
        t_rows[0],
        &[
            "T-001",
            "paper-stack",
            "Tune ball–chain damping params",
            "doing",
            "high",
            "me",
            "feat/damping-sweep"
        ]
    );

    // Summary: 1 project · 10 tasks
    assert!(
        out.contains(
            "Summary: 1 project · 10 tasks (1 todo, 1 doing, 1 follow-up, 0 blocked, 7 done)"
        ),
        "summary: {out}"
    );
}

#[test]
fn status_project_board_groups_by_all_statuses() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let out = status_project_board(&app, "paper-stack").unwrap();
    assert!(out.contains("Board: paper-stack"));
    assert!(out.contains("Todo\n----\n- T-002  Analyze restitution vs stack size"));
    assert!(out.contains("Doing\n-----\n- T-001  Tune ball–chain damping params"));
    assert!(out.contains("Follow-Up\n---------\n- T-003  Write methods section draft"));
    assert!(out.contains("Blocked\n-------\n- (none)"));
}

#[test]
fn status_by_key_and_name_are_identical() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let by_name = status_project(&app, "paper-stack").unwrap();
    let by_key = status_project(&app, "P-001").unwrap();
    assert_eq!(
        by_name, by_key,
        "`lun status paper-stack` and `lun status P-001` must be identical"
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
    let mut input = prompt_reader(&["todo", "med", "me", "", ""]);
    let out = create_task(&app, "my task title", Some("paper-stack"), &mut input).unwrap();
    let lines = out.split('\n').collect::<Vec<_>>();
    assert_eq!(lines[0], "Created task T-001 in project paper-stack");
    assert_eq!(
        lines[1],
        "Committed: add task \"my task title\" to paper-stack"
    );

    // Task exists in DB with the answered fields
    let task = app.lun.task_by_key("T-001").unwrap();
    assert_eq!(task.title, "my task title");
    assert_eq!(task.status, "todo");
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
    assert!(entries[0].details.contains("\"status\": \"todo\""));
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
    assert_eq!(lines[0], "Created task T-001 in project Unassigned");
    assert_eq!(lines[1], "Committed: add task \"bare task\" to Unassigned");

    let t = app.lun.task_by_key("T-001").unwrap();
    assert_eq!(t.status, "todo");
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
    let mut input = prompt_reader(&["doing", "high", "me", "custom commit message"]);
    let out = create_task(&app, "solo task", Some("solo"), &mut input).unwrap();
    assert_eq!(
        out.lines().next().unwrap(),
        "Created task T-001 in project solo"
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
    let mut input = prompt_reader(&["todo", "med", "", ""]);
    let out = create_task(&app, "unhome task", None, &mut input).unwrap();
    assert_eq!(
        out.lines().next().unwrap(),
        "Created task T-001 in project Unassigned"
    );
    assert_eq!(
        out.lines().nth(1).unwrap(),
        "Committed: add task \"unhome task\" to Unassigned"
    );
    let t = app.lun.task_by_key("T-001").unwrap();
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

    let mut input = prompt_reader(&["todo", "urgent", "", "", ""]);
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
    let mut input = prompt_reader(&["todo"]);
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

    // Header
    assert!(out.starts_with("Task T-003\n"), "header: {out}");
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
        out.contains("Status:    follow-up\n"),
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

    // Checklist / notes stubs
    assert!(
        out.contains("Checklist:\n- [ ] (add checklist items with `lun task edit T-003`)"),
        "checklist stub: {out}"
    );
    assert!(
        out.contains("Notes:\n- (add notes with `lun task edit T-003`)"),
        "notes stub: {out}"
    );

    // History (log): the CREATE entry with compact detail + commit line.
    assert!(out.contains("History (log):\n"), "history section: {out}");
    assert!(
        out.contains("  me  CREATE\n    Status: follow-up, Priority: high\n    Commit: add task \"Write methods section draft\" to paper-stack"),
        "CREATE history entry: {out}"
    );
}

#[test]
fn task_view_by_key() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let out = task_view(&app, "T-001").unwrap();
    assert!(out.starts_with("Task T-001\n"), "header: {out}");
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
    let out = status_target(&app, "Write methods section draft").unwrap();
    assert!(out.starts_with("**Task T-003**"), "header: {out}");
    assert!(out.contains("Project:   paper-stack"), "project: {out}");
    assert!(out.contains("Title:     Write methods section draft"), "title: {out}");
    assert!(out.contains("Status:    follow-up"), "status: {out}");
    assert!(out.contains("Priority:  high"), "priority: {out}");
    assert!(out.contains("Checklist:\n\n- [ ]"), "checklist: {out}");
    assert!(out.contains("**Last Commit:**"), "last commit section: {out}");
    assert!(
        out.contains("  me  CREATE\n    Status: follow-up, Priority: high\n    Commit: add task"),
        "last commit entry: {out}"
    );
    assert!(!out.contains("History (log):"), "should not show full history: {out}");
}

#[test]
fn task_list_edit_complete_reopen_and_archive_work() {
    let (_root, lun) = fixture();
    let app = App { lun };

    let listed = task_list(
        &app,
        &[
            "--project".into(),
            "paper-stack".into(),
            "--status".into(),
            "follow-up".into(),
            "--sort".into(),
            "title".into(),
        ],
    )
    .unwrap();
    assert!(listed.contains("T-003"));
    assert!(!listed.contains("T-001"));

    let mut input = prompt_reader(&[""]);
    let edited = task_edit(
        &app,
        "T-003",
        &[
            "--status".into(),
            "doing".into(),
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
    assert!(edited.contains("Updated T-003"));

    let out = task_view(&app, "T-003").unwrap();
    assert!(out.contains("Status:    doing"));
    assert!(out.contains("Priority:  med"));
    assert!(out.contains("Branch:    feat/edited"));
    assert!(out.contains("Labels:    [\"docs\", \"phase11\"]"));

    let mut input = prompt_reader(&[""]);
    let completed = task_complete(&app, "T-003", &mut input).unwrap();
    assert!(completed.contains("Completed T-003"));
    assert!(task_view(&app, "T-003")
        .unwrap()
        .contains("Status:    done"));

    let mut input = prompt_reader(&[""]);
    let reopened = task_reopen(&app, "T-003", &[], &mut input).unwrap();
    assert!(reopened.contains("Reopened T-003"));
    assert!(task_view(&app, "T-003")
        .unwrap()
        .contains("Status:    doing"));

    let mut input = prompt_reader(&[""]);
    let archived = task_archive(&app, "T-003", &mut input).unwrap();
    assert!(archived.contains("Archived T-003"));
    let archived_view = task_view(&app, "T-003").unwrap();
    assert!(archived_view.contains("Archived:  yes"));
    assert!(!task_list(&app, &[]).unwrap().contains("T-003"));
    assert!(task_list(&app, &["--all".into()])
        .unwrap()
        .contains("T-003"));
}

// ---------------------------------------------------------------------------
// lun log
// ---------------------------------------------------------------------------

#[test]
fn log_for_task_matches_plan_format() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let out = log_view(&app, "Write methods section draft").unwrap();

    // Compact commit-style entry with bullet marker.
    assert!(
        out.contains("- ") && out.contains("  me  CREATE\n    Status: follow-up, Priority: high"),
        "CREATE entry: {out}"
    );
}

#[test]
fn log_for_task_by_key() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let out = log_view(&app, "T-001").unwrap();
    assert!(out.contains("  me  CREATE"), "entry present: {out}");
}

#[test]
fn log_for_project_lists_task_history_newest_first() {
    let (_root, lun) = fixture();
    let app = App { lun };
    let out = log_view(&app, "paper-stack").unwrap();
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
        out.contains("Status: done, Priority: med"),
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
    let e = log_view(&app, "ghost").unwrap_err();
    assert_eq!(e.kind(), "not-found");
    let e = log_view(&app, "T-999").unwrap_err();
    assert_eq!(e.kind(), "not-found");
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
    assert_eq!(t.task_key, "T-003");
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
    let e = resolve_task(&lun, "same title").unwrap_err();
    assert_eq!(e.kind(), "ambiguous");
    assert!(e.to_string().contains("T-001, T-002"));
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
