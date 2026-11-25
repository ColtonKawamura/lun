//! The board's link feed (`Lun::links_by_task`, `GET /api/links`): one query
//! for the card link chip, oldest link first per task, task key attached, and
//! project links left out.

use lun::{LinkTarget, Lun, ProjectSpec, TaskSpec};
use std::path::PathBuf;

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lun-board-links-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn task(lun: &Lun, title: &str) -> lun::Task {
    lun.create_task(TaskSpec {
        title: title.into(),
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
    .unwrap()
}

#[test]
fn links_by_task_groups_by_task_and_keeps_insert_order() {
    let root = temp_root("order");
    let lun = Lun::init(&root).unwrap();
    let first = task(&lun, "first");
    let second = task(&lun, "second");
    let project = lun
        .create_project(ProjectSpec {
            name: "proj".into(),
            status: Some("active".into()),
            message: None,
            user: None,
        })
        .unwrap();

    lun.add_link(LinkTarget::Task(first.id), "Design", "https://a.example", None, None)
        .unwrap();
    lun.add_link(LinkTarget::Task(first.id), "Mail", "message:%3Cabc@example.com%3E", None, None)
        .unwrap();
    lun.add_link(LinkTarget::Task(second.id), "Site", "https://b.example", None, None)
        .unwrap();
    // A project link is not on the board's card feed.
    lun.add_link(LinkTarget::Project(project.id), "Spec", "https://c.example", None, None)
        .unwrap();

    let rows = lun.links_by_task().unwrap();
    let got: Vec<(String, String, String)> = rows
        .into_iter()
        .map(|(key, link)| (key, link.label, link.uri))
        .collect();
    assert_eq!(
        got,
        vec![
            (first.task_key.clone(), "Design".to_string(), "https://a.example".to_string()),
            (first.task_key.clone(), "Mail".to_string(), "message:%3Cabc@example.com%3E".to_string()),
            (second.task_key.clone(), "Site".to_string(), "https://b.example".to_string()),
        ]
    );
}

#[test]
fn links_by_task_is_empty_without_links() {
    let root = temp_root("empty");
    let lun = Lun::init(&root).unwrap();
    task(&lun, "no links");
    assert!(lun.links_by_task().unwrap().is_empty());
}

#[test]
fn added_link_is_logged_as_link() {
    let root = temp_root("log");
    let lun = Lun::init(&root).unwrap();
    let t = task(&lun, "card");
    lun.add_link(LinkTarget::Task(t.id), "Mail", "message:%3Cabc@example.com%3E", None, Some("web"))
        .unwrap();
    let logs = lun.task_logs(t.id).unwrap();
    assert_eq!(logs[0].action, "LINK");
    assert!(logs[0].message.contains("message:%3Cabc@example.com%3E"));
}
