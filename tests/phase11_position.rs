//! Phase 11 (schema v8) tests: explicit within-column `position`,
//! reorder_task swaps, archive/unarchive round-trip, and the v7->v8
//! upgrade path with position seeding.

use lun::db::{TaskListSpec, TaskSort};
use lun::{Lun, TaskSpec};

fn temp_root(name: &str) -> std::path::PathBuf {
    // Incorporate a high‑resolution timestamp to guarantee uniqueness even when
    // multiple tests in the same binary run concurrently (they share the same
    // process ID). This avoids directory collisions that previously caused
    // nondeterministic ordering failures.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "lun-test-{}-{}-{}",
        name,
        std::process::id(),
        ts
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn board_list(lun: &Lun) -> Vec<String> {
    let spec = TaskListSpec {
        project_id: None,
        status: None,
        priority: None,
        assignee: None,
        scope: None,
        include_archived: false,
        sort: TaskSort::Board,
    };
    lun.list_tasks_with(&spec)
        .unwrap()
        .into_iter()
        .map(|t| t.task_key)
        .collect()
}


/// Build an expected-key Vec from references (avoids moving task keys).
fn expected(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|s| s.to_string()).collect()
}

/// Read the raw `position` column for a task key.
fn position_of(root: &std::path::Path, key: &str) -> Option<i64> {
    let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
    conn.query_row(
        "SELECT position FROM tasks WHERE task_key = ?1",
        [key],
        |r| r.get(0),
    )
    .unwrap()
}

fn make(lun: &Lun, title: &str, status: &str) -> lun::db::Task {
    lun.create_task(TaskSpec {
        title: title.to_string(),
        status: Some(status.to_string()),
        project: None,
        priority: None,
        assignee: None,
        branch: None,
        labels: None,
        scope: None,
        message: None,
        user: None,
    })
    .unwrap()
}

#[test]
fn new_tasks_get_incrementing_position_in_column() {
    let root = temp_root("pos-new");
    let lun = Lun::init(&root).unwrap();
    let a = make(&lun, "alpha", "planning");
    let b = make(&lun, "beta", "planning");
    let c = make(&lun, "gamma", "review");
    println!("positions: a={:?}, b={:?}, c={:?}", position_of(&root, &a.task_key), position_of(&root, &b.task_key), position_of(&root, &c.task_key));
    println!("board order: {:?}", board_list(&lun));

    let pa: Option<i64> = position_of(&root, &a.task_key);
    let pb: Option<i64> = position_of(&root, &b.task_key);
    let pc: Option<i64> = position_of(&root, &c.task_key);

    assert!(pa.is_some() && pb.is_some() && pc.is_some(), "position assigned");
    assert!(pb.unwrap() > pa.unwrap(), "second planning card after first");
    assert_eq!(pc.unwrap(), 1, "first card in a column starts at 1");

    let keys = board_list(&lun);
    assert_eq!(keys[0], a.task_key);
    assert_eq!(keys[1], b.task_key);
    assert_eq!(keys[2], c.task_key);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reorder_task_swaps_with_neighbor_and_back() {
    let root = temp_root("pos-reorder");
    let lun = Lun::init(&root).unwrap();
    let a = make(&lun, "alpha", "planning");
    let b = make(&lun, "beta", "planning");
    let _c = make(&lun, "gamma", "planning");

    assert_eq!(board_list(&lun), expected(&[&a.task_key, &b.task_key, &_c.task_key]));

    // Move beta down: alpha, gamma, beta.
    lun.reorder_task(b.id, 1).unwrap();
    assert_eq!(board_list(&lun), expected(&[&a.task_key, &_c.task_key, &b.task_key]));

    // Move beta back up: alpha, beta, gamma.
    lun.reorder_task(b.id, -1).unwrap();
    assert_eq!(board_list(&lun), expected(&[&a.task_key, &b.task_key, &_c.task_key]));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reorder_at_column_edge_is_noop() {
    let root = temp_root("pos-edge");
    let lun = Lun::init(&root).unwrap();
    let a = make(&lun, "alpha", "planning");
    let _b = make(&lun, "beta", "planning");

    // Already at the top: moving up changes nothing.
    let before = board_list(&lun);
    lun.reorder_task(a.id, -1).unwrap();
    assert_eq!(board_list(&lun), before);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reorder_respects_column_boundary() {
    let root = temp_root("pos-boundary");
    let lun = Lun::init(&root).unwrap();
    let a = make(&lun, "alpha", "planning");
    let b = make(&lun, "beta", "review");

    // beta is the only card in `review`: moving down must not pull a card
    // from another column.
    let before = board_list(&lun);
    lun.reorder_task(b.id, 1).unwrap();
    assert_eq!(board_list(&lun), before);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn archive_unarchive_roundtrip() {
    let root = temp_root("pos-archive");
    let lun = Lun::init(&root).unwrap();
    let a = make(&lun, "alpha", "planning");
    let _b = make(&lun, "beta", "planning");

    lun.archive_task(a.id, None, Some("test")).unwrap();
    let keys = board_list(&lun);
    assert_eq!(keys, expected(&[_b.task_key.as_str()]), "archived task hidden from board list");

    lun.unarchive_task(a.id, Some("test")).unwrap();
    let keys = board_list(&lun);
    assert_eq!(keys, expected(&[a.task_key.as_str(), _b.task_key.as_str()]), "unarchived task restored");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v7_database_upgrades_to_v8_seeding_position() {
    let root = temp_root("pos-upgrade");
    let db_path = root.join(".lun");
    std::fs::create_dir_all(&db_path).unwrap();
    let conn = rusqlite::Connection::open(db_path.join("lun.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE projects (
            id INTEGER PRIMARY KEY, project_key TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
         CREATE TABLE tasks (
            id INTEGER PRIMARY KEY, task_key TEXT NOT NULL UNIQUE,
            project_id INTEGER, title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'todo',
            priority TEXT NOT NULL DEFAULT 'low', assignee TEXT, branch TEXT,
            labels TEXT NOT NULL DEFAULT '[]', scope TEXT,
            notes TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
            archived_at TEXT);
         CREATE TABLE logs (
            id INTEGER PRIMARY KEY, entity_type TEXT NOT NULL,
            entity_id INTEGER NOT NULL, timestamp TEXT NOT NULL,
            user TEXT NOT NULL, action TEXT NOT NULL,
            message TEXT NOT NULL, details TEXT NOT NULL DEFAULT '{}');
         CREATE TABLE attachments (
            id INTEGER PRIMARY KEY, task_id INTEGER, project_id INTEGER,
            filename TEXT NOT NULL, stored_path TEXT NOT NULL,
            created_at TEXT NOT NULL);
         CREATE TABLE links (
            id INTEGER PRIMARY KEY, task_id INTEGER, project_id INTEGER,
            label TEXT NOT NULL, uri TEXT NOT NULL, created_at TEXT NOT NULL);
         CREATE TABLE prs (
            id INTEGER PRIMARY KEY, pr_key TEXT NOT NULL UNIQUE,
            task_id INTEGER NOT NULL, source_branch TEXT NOT NULL,
            target_branch TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'open',
            created_at TEXT NOT NULL, merged_at TEXT);
         CREATE TABLE migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
         INSERT INTO migrations (version, applied_at) VALUES (7, '2026-01-01T00:00:00Z');
         INSERT INTO projects (project_key, name, status, created_at, updated_at)
             VALUES ('P-000', 'Unassigned', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO tasks (task_key, project_id, title, status, created_at, updated_at)
             VALUES
               ('T-003', 1, 'charlie', 'todo', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
               ('T-001', 1, 'alpha',   'todo', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
               ('T-002', 1, 'beta',    'follow-up', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
    )
    .unwrap();
    drop(conn);

    // Opening with the new binary must run the v8 migration.
    let lun = Lun::init(&root).unwrap();
    let keys = board_list(&lun);
    // todo -> planning keeps task_key order (seeded positions); follow-up -> review after.
    assert_eq!(keys, [
        "T-001".to_string(),
        "T-003".to_string(),
        "T-002".to_string(),
    ]);

    let version: i64 = rusqlite::Connection::open(root.join(".lun/lun.db"))
        .unwrap()
        .query_row("SELECT MAX(version) FROM migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, lun::db::CURRENT_VERSION);

    let _ = std::fs::remove_dir_all(&root);
}
