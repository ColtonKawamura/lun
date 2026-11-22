//! Deleting a link logs UNLINK; restore re-adds the newest un-restored one.

use lun::{LinkTarget, Lun, TaskSpec};

fn fixture(name: &str) -> (Lun, i64) {
    let root = std::env::temp_dir().join(format!("lun-linkdel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let lun = Lun::init(&root).unwrap();
    let t = lun
        .create_task(TaskSpec {
            title: "t".into(),
            project: None,
            status: Some("planning".into()),
            priority: Some("med".into()),
            assignee: Some("me".into()),
            branch: None,
            labels: None,
            message: None,
            scope: None,
            user: None,
        })
        .unwrap();
    (lun, t.id)
}

#[test]
fn delete_logs_unlink_and_restore_brings_it_back() {
    let (lun, tid) = fixture("a");
    let a = lun.add_link(LinkTarget::Task(tid), "A", "hermes://open/a", None, None).unwrap();
    lun.add_link(LinkTarget::Task(tid), "B", "https://b \"q\"", None, None).unwrap();

    lun.delete_link(a, None).unwrap();
    assert_eq!(lun.links_for_task(tid).unwrap().len(), 1);
    let logs = lun.logs_for("task", tid).unwrap();
    assert_eq!(logs[0].action, "UNLINK");
    assert!(logs[0].details.contains("hermes://open/a"));

    let restored = lun.restore_last_link(LinkTarget::Task(tid), None).unwrap().unwrap();
    assert_eq!((restored.label.as_str(), restored.uri.as_str()), ("A", "hermes://open/a"));
    assert_eq!(lun.links_for_task(tid).unwrap().len(), 2);
    // Already restored: nothing left to restore.
    assert!(lun.restore_last_link(LinkTarget::Task(tid), None).unwrap().is_none());
}

#[test]
fn restore_is_newest_first_and_handles_awkward_labels() {
    let (lun, tid) = fixture("b");
    let a = lun.add_link(LinkTarget::Task(tid), "A \"x\"", "https://a", None, None).unwrap();
    let b = lun.add_link(LinkTarget::Task(tid), "B", "https://b", None, None).unwrap();
    lun.delete_link(a, None).unwrap();
    lun.delete_link(b, None).unwrap();
    let first = lun.restore_last_link(LinkTarget::Task(tid), None).unwrap().unwrap();
    assert_eq!(first.uri, "https://b");
    let second = lun.restore_last_link(LinkTarget::Task(tid), None).unwrap().unwrap();
    assert_eq!((second.label.as_str(), second.uri.as_str()), ("A \"x\"", "https://a"));
    assert!(lun.restore_last_link(LinkTarget::Task(tid), None).unwrap().is_none());
}

#[test]
fn nothing_to_restore() {
    let (lun, tid) = fixture("c");
    assert!(lun.restore_last_link(LinkTarget::Task(tid), None).unwrap().is_none());
}
