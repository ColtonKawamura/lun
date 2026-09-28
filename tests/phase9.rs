//! Phase 9 tests: PRs (GitHub-style workflow) and git glue.
//!
//! Covers the plan's Phase 9 guarantees:
//! - `prs` table (schema v3) with sequenced `PR-00N` keys (MAX(id)-based).
//! - `create_pr` — source defaults to the task's `branch`, target to
//!   `main`; one open PR per task; source == target rejected; tasks with
//!   no branch require `--from`.
//! - `merge_pr` — PR -> `merged`, task -> `done`, `MERGE` log entry;
//!   merging twice is an error.
//! - Log-on-write: opening a PR writes exactly one `UPDATE` entry on the
//!   task carrying the PR key in `details`; merging writes exactly one
//!   `MERGE` entry (the "map logs entries to the PR lifecycle" step).
//! - `git merge` glue never fails the command (logical merge is canonical).
//! - CLI views/resolution (`pr_ls`, `pr_show`, `task_view`, `resolve_pr`)
//!   and dispatch via `lun::cli::run` with the standard exit codes
//!   (2 = usage/resolution, 1 = runtime, 0 = success).

use lun::cli::{App, EXIT_USAGE};
use lun::{Lun, PrSpec, ProjectSpec, Task, TaskSpec};
use std::path::PathBuf;

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lun-p9-test-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Fixture: project "lun-cli" (P-001) with T-001 (has a branch) and T-002
/// (no branch) — the two shapes `lun pr new` must handle.
fn fixture() -> (PathBuf, Lun, Task, Task) {
    let root = temp_root("fx9");
    let lun = Lun::init(&root).unwrap();
    let p = lun
        .create_project(ProjectSpec {
            name: "lun-cli".into(),
            ..Default::default()
        })
        .unwrap();
    let t01 = lun
        .create_task(TaskSpec {
            title: "ship the PR workflow".into(),
            project: Some(p.id),
            status: Some("in-progress".into()),
            priority: Some("high".into()),
            assignee: Some("me".into()),
            branch: Some("pr9-sim-branch".into()),
            ..Default::default()
        })
        .unwrap();
    let t02 = lun
        .create_task(TaskSpec {
            title: "branchless task".into(),
            project: Some(p.id),
            status: Some("todo".into()),
            priority: Some("med".into()),
            assignee: Some("me".into()),
            branch: None,
            ..Default::default()
        })
        .unwrap();
    (root, lun, t01, t02)
}

fn open_pr(lun: &Lun, task_id: i64) -> lun::Pr {
    lun.create_pr(PrSpec {
        task_id,
        ..Default::default()
    })
    .unwrap()
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[test]
fn schema_includes_prs_table() {
    let (root, lun, _, _) = fixture();
    assert!(lun::db::CURRENT_VERSION >= 3);
    // A fresh current-version DB has no PRs but the table answers.
    assert!(lun.list_prs().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// create_pr
// ---------------------------------------------------------------------------

#[test]
fn create_pr_defaults_source_to_task_branch_and_target_to_main() {
    let (root, lun, t01, _) = fixture();
    let before = lun.logs_for("task", t01.id).unwrap().len();
    let pr = open_pr(&lun, t01.id);
    assert_eq!(pr.pr_key, "PR-001");
    assert_eq!(pr.source_branch, "pr9-sim-branch");
    assert_eq!(pr.target_branch, "main");
    assert_eq!(pr.status, "open");
    assert!(pr.merged_at.is_none());
    // Log-on-write: exactly one new task log entry, an UPDATE carrying the
    // PR key in details (the PR lifecycle lives in the task's log).
    // `logs_for` is newest-first, so the new entry is `first()`.
    let logs = lun.logs_for("task", t01.id).unwrap();
    assert_eq!(logs.len(), before + 1);
    let e = logs.first().unwrap();
    assert_eq!(e.action, "UPDATE");
    assert!(e.details.contains("\"pr\": \"PR-001\""));
    assert!(e.details.contains("\"source\": \"pr9-sim-branch\""));
    assert!(e.details.contains("\"target\": \"main\""));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn create_pr_requires_branch_when_task_has_none() {
    let (root, lun, _, t02) = fixture();
    let err = lun
        .create_pr(PrSpec {
            task_id: t02.id,
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.kind(), "usage");
    assert!(err.to_string().contains("no branch"));
    // Explicit source is the escape hatch.
    let pr = lun
        .create_pr(PrSpec {
            task_id: t02.id,
            source_branch: Some("explicit-branch".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pr.source_branch, "explicit-branch");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn create_pr_rejects_source_equal_to_target() {
    let (root, lun, _, t02) = fixture();
    let err = lun
        .create_pr(PrSpec {
            task_id: t02.id,
            source_branch: Some("main".into()),
            target_branch: Some("main".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.kind(), "usage");
    assert!(err.to_string().contains("source and target"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn one_open_pr_per_task() {
    let (root, lun, t01, _) = fixture();
    open_pr(&lun, t01.id);
    let err = lun
        .create_pr(PrSpec {
            task_id: t01.id,
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.kind(), "usage");
    assert!(err.to_string().contains("already has an open PR"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pr_keys_are_sequenced_max_based_across_merge() {
    let (root, lun, t01, _) = fixture();
    let p1 = open_pr(&lun, t01.id);
    assert_eq!(p1.pr_key, "PR-001");
    lun.merge_pr(p1.id, None, None).unwrap();
    // Re-opening after a merge is allowed and continues the sequence.
    let p2 = open_pr(&lun, t01.id);
    assert_eq!(p2.pr_key, "PR-002");
    assert_eq!(lun.list_open_prs().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// merge_pr
// ---------------------------------------------------------------------------

#[test]
fn merge_pr_marks_pr_merged_and_task_done_with_merge_log() {
    let (root, lun, t01, _) = fixture();
    let pr = open_pr(&lun, t01.id);
    let before = lun.logs_for("task", t01.id).unwrap().len();
    let merged = lun.merge_pr(pr.id, None, None).unwrap();
    assert_eq!(merged.status, "merged");
    assert!(merged.merged_at.is_some());
    assert_eq!(lun.task_by_id(t01.id).unwrap().status, "done");
    // Exactly one new log entry: the MERGE with the PR key in details
    // (newest-first, so `first()`).
    let logs = lun.logs_for("task", t01.id).unwrap();
    assert_eq!(logs.len(), before + 1);
    let e = logs.first().unwrap();
    assert_eq!(e.action, "MERGE");
    assert_eq!(e.message, "merge PR-001 (pr9-sim-branch -> main)");
    assert!(e.details.contains("\"pr\": \"PR-001\""));
    assert!(e.details.contains("done"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn merge_pr_twice_is_an_error() {
    let (root, lun, t01, _) = fixture();
    let pr = open_pr(&lun, t01.id);
    lun.merge_pr(pr.id, None, None).unwrap();
    let err = lun.merge_pr(pr.id, None, None).unwrap_err();
    assert_eq!(err.kind(), "usage");
    assert!(err.to_string().contains("already merged"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn merge_missing_pr_is_not_found() {
    let (root, lun, _, _) = fixture();
    let err = lun.merge_pr(999, None, None).unwrap_err();
    assert_eq!(err.kind(), "not-found");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// CLI resolution + views
// ---------------------------------------------------------------------------

#[test]
fn resolve_pr_by_key_and_by_task() {
    let (root, lun, t01, t02) = fixture();
    let app = App { lun };
    let pr = open_pr(&app.lun, t01.id);
    // By key.
    assert_eq!(lun::cli::resolve_pr(&app, &pr.pr_key).unwrap().id, pr.id);
    // By task key (the task's single open PR).
    assert_eq!(lun::cli::resolve_pr(&app, &t01.task_key).unwrap().id, pr.id);
    // A task with no open PR is a clean not-found.
    let err = lun::cli::resolve_pr(&app, &t02.task_key).unwrap_err();
    assert_eq!(err.kind(), "not-found");
    // A bogus PR key is a not-found, not a panic.
    let err = lun::cli::resolve_pr(&app, "PR-999").unwrap_err();
    assert_eq!(err.kind(), "not-found");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pr_ls_lists_open_before_merged() {
    let (root, lun, t01, t02) = fixture();
    let app = App { lun };
    // Empty state.
    let out = lun::cli::pr_ls(&app).unwrap();
    assert!(out.contains("No PRs yet"));
    // Open one on each task, merge T-002's, then list: open (T-001) first,
    // merged (T-002) after.
    open_pr(&app.lun, t01.id);
    let p2 = app
        .lun
        .create_pr(PrSpec {
            task_id: t02.id,
            source_branch: Some("explicit-branch".into()),
            ..Default::default()
        })
        .unwrap();
    app.lun.merge_pr(p2.id, None, None).unwrap();
    let out = lun::cli::pr_ls(&app).unwrap();
    let open_pos = out.find("Open PRs").expect("open section");
    let merged_pos = out.find("Merged PRs").expect("merged section");
    assert!(open_pos < merged_pos);
    assert!(out.contains(&t01.task_key));
    assert!(out.contains(&t02.task_key));
    assert!(out.contains("pr9-sim-branch -> main"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn task_view_shows_prs_section() {
    let (root, lun, t01, _) = fixture();
    let app = App { lun };
    // No PRs yet: a hint line.
    let out = lun::cli::task_view(&app, &t01.task_key).unwrap();
    assert!(out.contains("PRs:"));
    assert!(out.contains("no PRs"));
    // After opening: the PR row with its branches.
    open_pr(&app.lun, t01.id);
    let out = lun::cli::task_view(&app, &t01.task_key).unwrap();
    assert!(out.contains("PR-001"));
    assert!(out.contains("pr9-sim-branch -> main"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pr_show_renders_pr_details_and_history() {
    let (root, lun, t01, _) = fixture();
    let app = App { lun };
    let pr = open_pr(&app.lun, t01.id);
    app.lun.merge_pr(pr.id, None, None).unwrap();
    let out = lun::cli::pr_show(&app, &pr.pr_key).unwrap();
    assert!(out.contains(&format!("PR {}", pr.pr_key)));
    assert!(out.contains("Status:    merged"));
    assert!(out.contains("Merged:"));
    assert!(out.contains("History (log):"));
    // Both lifecycle entries are the task's UPDATE + MERGE (same details.pr).
    assert!(out.contains("UPDATE"));
    assert!(out.contains("MERGE"));
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// CLI dispatch (run -> exit codes)
// ---------------------------------------------------------------------------

#[test]
fn run_dispatch_pr_subcommands() {
    let (root, lun, t01, _) = fixture();
    let app = App { lun };
    let success = std::process::ExitCode::SUCCESS;
    let usage = std::process::ExitCode::from(EXIT_USAGE);

    // `pr` with no subcommand is a usage error.
    let args: Vec<String> = vec!["pr".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    // `pr ls` on the empty DB succeeds.
    let args: Vec<String> = vec!["pr".into(), "ls".into()];
    assert_eq!(lun::cli::run(&app, &args), success);
    // `pr new T-001` (defaults: task's branch -> main).
    let args: Vec<String> = vec!["pr".into(), "new".into(), "T-001".into()];
    assert_eq!(lun::cli::run(&app, &args), success);
    // Second open PR on the same task: usage error.
    let args: Vec<String> = vec!["pr".into(), "new".into(), "T-001".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    // `pr show` by key.
    let args: Vec<String> = vec!["pr".into(), "show".into(), "PR-001".into()];
    assert_eq!(lun::cli::run(&app, &args), success);
    // `pr merge` (logical merge; git glue only reports).
    let args: Vec<String> = vec!["pr".into(), "merge".into(), "PR-001".into()];
    assert_eq!(lun::cli::run(&app, &args), success);
    // Merging again: usage error.
    let args: Vec<String> = vec!["pr".into(), "merge".into(), "PR-001".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    // Unknown subcommand: usage error.
    let args: Vec<String> = vec!["pr".into(), "close".into(), "PR-001".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    // Missing PR key: not-found -> usage exit.
    let args: Vec<String> = vec!["pr".into(), "show".into(), "PR-999".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_pr_new_with_explicit_flags() {
    let (root, lun, _, t02) = fixture();
    let app = App { lun };
    let args: Vec<String> = vec![
        "pr".into(),
        "new".into(),
        t02.task_key.clone(),
        "--from".into(),
        "feat/explicit".into(),
        "--to".into(),
        "develop".into(),
    ];
    assert_eq!(lun::cli::run(&app, &args), std::process::ExitCode::SUCCESS);
    let prs = app.lun.list_prs().unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].source_branch, "feat/explicit");
    assert_eq!(prs[0].target_branch, "develop");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_pr_new_missing_task_is_usage() {
    let (root, lun, _, _) = fixture();
    let app = App { lun };
    let usage = std::process::ExitCode::from(EXIT_USAGE);
    let args: Vec<String> = vec!["pr".into(), "new".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    // `--from` without a branch value is a usage error.
    let args: Vec<String> = vec!["pr".into(), "new".into(), "T-001".into(), "--from".into()];
    assert_eq!(lun::cli::run(&app, &args), usage);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn merge_glue_never_fails_the_command_outside_a_repo() {
    // `pr_merge` with a non-git root: the logical merge still lands and no
    // git note is emitted.
    let (root, lun, t01, _) = fixture();
    let pr = open_pr(&lun, t01.id);
    let scratch = temp_root("not-a-repo");
    let app = App { lun };
    let out = lun::cli::pr_merge(&app, &pr.pr_key, &scratch).unwrap();
    assert!(out.contains(&format!("Merged {}", pr.pr_key)));
    assert!(!out.contains("git:"));
    assert_eq!(app.lun.task_by_id(t01.id).unwrap().status, "done");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&scratch);
}
