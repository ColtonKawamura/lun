//! Phase 2 tests: init idempotency, key generation, log-on-write,
//! P-000 fallback, and attachment/link records.

use lun::{Lun, LinkTarget, ProjectSpec, TaskSpec};

fn temp_root(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lun-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Open a second connection to the on-disk DB for independent assertions.
fn side_conn(root: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap()
}

#[test]
fn init_creates_db_and_schema() {
    let root = temp_root("schema");
    let lun = Lun::init(&root).unwrap();

    assert!(root.join(".lun/lun.db").exists());

    let projects = lun.list_projects().unwrap();
    assert_eq!(projects.len(), 1, "only P-000 seeded");
    assert_eq!(projects[0].project_key, "P-000");
    assert_eq!(projects[0].name, "Unassigned");

    let conn = side_conn(&root);
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'
              AND name IN ('projects','tasks','logs','attachments','links','migrations')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 6, "all five tables + migrations present");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn init_is_idempotent() {
    let root = temp_root("idempotent");
    let lun1 = Lun::init(&root).unwrap();
    let p = lun1
        .create_project(ProjectSpec {
            name: "alpha".into(),
            ..Default::default()
        })
        .unwrap();
    let t = lun1
        .create_task(TaskSpec {
            title: "first task".into(),
            ..Default::default()
        })
        .unwrap();

    // Re-run init on the same directory: nothing lost, nothing duplicated.
    let lun2 = Lun::init(&root).unwrap();
    let projects = lun2.list_projects().unwrap();
    let tasks = lun2.list_tasks().unwrap();
    assert_eq!(projects.len(), 2, "P-000 + alpha");
    assert_eq!(
        projects.iter().find(|x| x.id == p.id).unwrap().name,
        "alpha"
    );
    assert_eq!(tasks.len(), 1, "first task still there, not duplicated");
    assert_eq!(tasks[0].task_key, t.task_key);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn project_keys_auto_increment() {
    let root = temp_root("projkeys");
    let lun = Lun::init(&root).unwrap();

    let keys: Vec<String> = (0..3)
        .map(|i| {
            lun.create_project(ProjectSpec {
                name: format!("p{i}"),
                ..Default::default()
            })
            .unwrap()
            .project_key
        })
        .collect();
    assert_eq!(keys, vec!["P-001", "P-002", "P-003"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn task_keys_auto_increment() {
    let root = temp_root("taskkeys");
    let lun = Lun::init(&root).unwrap();

    let keys: Vec<String> = (0..3)
        .map(|i| {
            lun.create_task(TaskSpec {
                title: format!("task {i}"),
                ..Default::default()
            })
            .unwrap()
            .task_key
        })
        .collect();
    assert_eq!(keys, vec!["T-001", "T-002", "T-003"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn task_without_project_falls_back_to_p000() {
    let root = temp_root("p000");
    let lun = Lun::init(&root).unwrap();

    let t = lun
        .create_task(TaskSpec {
            title: "orphan".into(),
            project: None,
            ..Default::default()
        })
        .unwrap();

    let pid = lun.unassigned_project_id().unwrap();
    assert_eq!(t.project_id, Some(pid), "orphan task points at P-000");
    let name: String = {
        let conn = side_conn(&root);
        conn.query_row("SELECT name FROM projects WHERE id = ?1", [pid], |r| {
            r.get(0)
        })
        .unwrap()
    };
    assert_eq!(name, "Unassigned");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn log_entry_written_for_every_state_change() {
    let root = temp_root("logonwrite");
    let lun = Lun::init(&root).unwrap();
    let conn = side_conn(&root);
    let count = |sql: &str| -> i64 {
        conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap()
    };

    // The seed write itself is logged.
    assert_eq!(
        count("SELECT COUNT(*) FROM logs WHERE entity_type='project' AND entity_id=1 AND action='CREATE'"),
        1,
        "P-000 seeding is logged"
    );

    // Create project -> exactly one log row on that project.
    let p = lun
        .create_project(ProjectSpec {
            name: "alpha".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM logs WHERE entity_type='project' AND entity_id=?1",
            [p.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "create_project wrote exactly one log row"
    );

    // Create task -> exactly one log row on that task, commit-style message.
    let t = lun
        .create_task(TaskSpec {
            title: "do it".into(),
            project: Some(p.id),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM logs WHERE entity_type='task' AND entity_id=?1",
            [t.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "create_task wrote exactly one log row"
    );
    let msg: String = conn
        .query_row(
            "SELECT message FROM logs WHERE entity_type='task' AND entity_id=?1 AND action='CREATE'",
            [t.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(msg, "add task \"do it\" to alpha");

    // Attachment -> exactly one ATTACH log row.
    lun.add_attachment(
        t.id,
        "notes.txt",
        ".lun/attachments/T-001-notes.txt",
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM logs WHERE entity_type='task' AND entity_id=?1 AND action='ATTACH'",
            [t.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "add_attachment wrote exactly one ATTACH log row"
    );

    // Link on a project -> exactly one LINK log row.
    lun.add_link(
        LinkTarget::Project(p.id),
        "obsidian",
        "obsidian://open?vault=personal",
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM logs WHERE entity_type='project' AND entity_id=?1 AND action='LINK'",
            [p.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "add_link wrote exactly one LINK log row"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn open_without_init_is_an_error() {
    let root = temp_root("noinit");
    let err = Lun::open(&root).unwrap_err();
    assert!(err.to_string().contains("lun init"), "{}", err);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn links_attach_to_tasks_and_projects() {
    let root = temp_root("linkcheck");
    let lun = Lun::init(&root).unwrap();
    let p = lun
        .create_project(ProjectSpec {
            name: "alpha".into(),
            ..Default::default()
        })
        .unwrap();
    let t = lun
        .create_task(TaskSpec {
            title: "t".into(),
            project: Some(p.id),
            ..Default::default()
        })
        .unwrap();

    // Both target types work.
    assert!(lun
        .add_link(LinkTarget::Task(t.id), "l1", "obsidian://x", None, None)
        .is_ok());
    assert!(lun
        .add_link(
            LinkTarget::Project(p.id),
            "l2",
            "https://example.com",
            None,
            None
        )
        .is_ok());

    // Unknown task id for an attachment is a clean error, not a panic.
    let err = lun
        .add_attachment(9999, "x.txt", ".lun/attachments/x.txt", None, None)
        .unwrap_err();
    assert!(err.to_string().contains("task 9999"), "{}", err);
    let _ = std::fs::remove_dir_all(&root);
}
