use lun::cli::{complete_output, App};
use lun::{Lun, ProjectSpec, TaskSpec};
use std::path::PathBuf;
use std::process::Command;

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lun-p11-test-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[test]
fn complete_top_level_empty_and_partial_prefix() {
    let root = temp_root("top-level");
    let _lun = Lun::init(&root).unwrap();
    let app = App::open(&root).unwrap();

    let empty = complete_output(
        Some(&app),
        &["--".into(), "lun".into(), "".into()],
    );
    let empty_lines = lines(&empty);
    assert!(empty_lines.iter().any(|s| s == "task"));
    assert!(empty_lines.iter().any(|s| s == "new"));

    let partial = complete_output(Some(&app), &["--".into(), "lun".into(), "ta".into()]);
    let partial_lines = lines(&partial);
    assert!(partial_lines.iter().any(|s| s == "task"), "{partial}");
    assert!(partial_lines.iter().all(|s| s.starts_with("ta")), "{partial}");

    let status_partial_with_empty = complete_output(
        Some(&app),
        &["--".into(), "lun".into(), "sta".into(), "".into()],
    );
    let status_partial_lines = lines(&status_partial_with_empty);
    assert!(
        status_partial_lines.iter().any(|s| s == "status"),
        "{status_partial_with_empty}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn complete_task_key_and_title_after_task_command() {
    let root = temp_root("task-values");
    let lun = Lun::init(&root).unwrap();
    let p = lun
        .create_project(ProjectSpec {
            name: "alpha-proj".into(),
            ..Default::default()
        })
        .unwrap();
    let task = lun
        .create_task(TaskSpec {
            title: "write completion docs".into(),
            project: Some(p.id),
            ..Default::default()
        })
        .unwrap();
    let app = App::open(&root).unwrap();

    let out = complete_output(
        Some(&app),
        &["--".into(), "lun".into(), "task".into(), "".into()],
    );
    let out_lines = lines(&out);
    assert!(out_lines.iter().any(|s| s == &task.task_key), "{out}");
    assert!(out_lines.iter().any(|s| s == &task.title), "{out}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn complete_project_after_proj_command() {
    let root = temp_root("project-values");
    let lun = Lun::init(&root).unwrap();
    let project = lun
        .create_project(ProjectSpec {
            name: "project with spaces".into(),
            ..Default::default()
        })
        .unwrap();
    let app = App::open(&root).unwrap();

    let out = complete_output(
        Some(&app),
        &["--".into(), "lun".into(), "proj".into(), "".into()],
    );
    let out_lines = lines(&out);
    assert!(out_lines.iter().any(|s| s == &project.project_key), "{out}");
    assert!(out_lines.iter().any(|s| s == &project.name), "{out}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn complete_status_and_log_targets_include_projects_and_tasks() {
    let root = temp_root("status-log-targets");
    let lun = Lun::init(&root).unwrap();
    let project = lun
        .create_project(ProjectSpec {
            name: "my Project".into(),
            ..Default::default()
        })
        .unwrap();
    let task = lun
        .create_task(TaskSpec {
            title: "myTask".into(),
            project: Some(project.id),
            ..Default::default()
        })
        .unwrap();
    let app = App::open(&root).unwrap();

    let status_out = complete_output(
        Some(&app),
        &["--".into(), "lun".into(), "status".into(), "".into()],
    );
    let status_lines = lines(&status_out);
    assert!(status_lines.iter().any(|s| s == &project.project_key), "{status_out}");
    assert!(status_lines.iter().any(|s| s == &project.name), "{status_out}");
    assert!(status_lines.iter().any(|s| s == &task.task_key), "{status_out}");
    assert!(status_lines.iter().any(|s| s == &task.title), "{status_out}");

    let log_out = complete_output(
        Some(&app),
        &["--".into(), "lun".into(), "log".into(), "".into()],
    );
    let log_lines = lines(&log_out);
    assert!(log_lines.iter().any(|s| s == &project.project_key), "{log_out}");
    assert!(log_lines.iter().any(|s| s == &project.name), "{log_out}");
    assert!(log_lines.iter().any(|s| s == &task.task_key), "{log_out}");
    assert!(log_lines.iter().any(|s| s == &task.title), "{log_out}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn complete_status_values_for_task_and_project_contexts() {
    let root = temp_root("status-values");
    let lun = Lun::init(&root).unwrap();
    let task = lun
        .create_task(TaskSpec {
            title: "x".into(),
            ..Default::default()
        })
        .unwrap();
    let app = App::open(&root).unwrap();

    let task_status = complete_output(
        Some(&app),
        &[
            "--".into(),
            "lun".into(),
            "task".into(),
            task.task_key.clone(),
            "--status".into(),
            "".into(),
        ],
    );
    let task_lines = lines(&task_status);
    assert!(task_lines.iter().any(|s| s == "todo"), "{task_status}");
    assert!(task_lines.iter().any(|s| s == "done"), "{task_status}");
    assert!(!task_lines.iter().any(|s| s == "active"), "{task_status}");

    let project_status = complete_output(
        Some(&app),
        &[
            "--".into(),
            "lun".into(),
            "proj".into(),
            "P-000".into(),
            "--status".into(),
            "".into(),
        ],
    );
    let project_lines = lines(&project_status);
    assert!(project_lines.iter().any(|s| s == "active"), "{project_status}");
    assert!(project_lines.iter().any(|s| s == "inactive"), "{project_status}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn complete_outside_repo_exits_zero_and_returns_static_candidates() {
    let root = temp_root("outside-repo");
    let bin = env!("CARGO_BIN_EXE_lun");
    let output = Command::new(bin)
        .args(["complete", "--", "lun", "ta"])
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|s| s.trim() == "task"), "stdout:\n{stdout}");

    let _ = std::fs::remove_dir_all(&root);
}
