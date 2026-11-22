//! Schema v9 tests: task statuses are Colton's Obsidian kanban stages
//! (`planning`, `review`, `waiting-on`, `backlog`, `closed`), the v8 -> v9
//! upgrade maps old values and keeps every child row attached, and the
//! pre-v9 names still work as input aliases.

use lun::db::{TaskListSpec, TaskSort, TASK_STATUSES};
use lun::{Lun, TaskSpec, TaskUpdateSpec};
use std::process::Command;

fn temp_root(name: &str) -> std::path::PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
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

/// A schema-v8 database exactly as lun 0.1 (pre-v9) bootstrapped it, with
/// one task per old status and child rows (log, link, attachment, PR)
/// pointing at the tasks through real foreign keys.
fn make_v8_db(root: &std::path::Path) {
    let dir = root.join(".lun");
    std::fs::create_dir_all(&dir).unwrap();
    let conn = rusqlite::Connection::open(dir.join("lun.db")).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn.execute_batch(
        "CREATE TABLE migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
         CREATE TABLE projects (
            id          INTEGER PRIMARY KEY,
            project_key TEXT NOT NULL UNIQUE,
            name        TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'active'
                       CHECK (status IN ('active', 'inactive')),
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
         );
         CREATE TABLE tasks (
            id          INTEGER PRIMARY KEY,
            task_key    TEXT NOT NULL UNIQUE,
            project_id  INTEGER REFERENCES projects(id),
            title       TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'todo'
                       CHECK (status IN ('todo', 'doing', 'follow-up', 'blocked', 'done')),
            priority    TEXT NOT NULL DEFAULT 'low'
                       CHECK (priority IN ('low', 'med', 'high')),
            assignee    TEXT,
            branch      TEXT,
            labels      TEXT NOT NULL DEFAULT '[]',
            scope       TEXT,
            notes       TEXT NOT NULL DEFAULT '',
            position    INTEGER,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            archived_at TEXT
         );
         CREATE TABLE logs (
            id          INTEGER PRIMARY KEY,
            entity_type TEXT NOT NULL CHECK (entity_type IN ('project', 'task')),
            entity_id   INTEGER NOT NULL,
            timestamp   TEXT NOT NULL,
            user        TEXT NOT NULL,
            action      TEXT NOT NULL,
            message     TEXT NOT NULL,
            details     TEXT NOT NULL DEFAULT '{}'
         );
         CREATE TABLE attachments (
            id          INTEGER PRIMARY KEY,
            task_id     INTEGER REFERENCES tasks(id),
            project_id  INTEGER REFERENCES projects(id),
            filename    TEXT NOT NULL,
            stored_path TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            CHECK ((task_id IS NULL) <> (project_id IS NULL))
         );
         CREATE TABLE links (
            id          INTEGER PRIMARY KEY,
            task_id     INTEGER REFERENCES tasks(id),
            project_id  INTEGER REFERENCES projects(id),
            label       TEXT NOT NULL,
            uri         TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            CHECK ((task_id IS NULL) <> (project_id IS NULL))
         );
         CREATE TABLE prs (
            id            INTEGER PRIMARY KEY,
            pr_key        TEXT NOT NULL UNIQUE,
            task_id       INTEGER NOT NULL REFERENCES tasks(id),
            source_branch TEXT NOT NULL,
            target_branch TEXT NOT NULL,
            status        TEXT NOT NULL DEFAULT 'open'
                        CHECK (status IN ('open', 'merged')),
            created_at    TEXT NOT NULL,
            merged_at     TEXT
         );
         INSERT INTO migrations VALUES (8, '2026-01-01T00:00:00Z');
         INSERT INTO projects (project_key, name, status, created_at, updated_at)
             VALUES ('P-000', 'Unassigned', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                    ('P-001', 'soft pack',  'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO tasks (task_key, project_id, title, status, notes, position, created_at, updated_at)
             VALUES ('T-001', 2, 'was todo',      'todo',      '# agent-status\n\nscoping', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                    ('T-002', 2, 'was doing',     'doing',     '', 2, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                    ('T-003', 2, 'was follow-up', 'follow-up', '', 3, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                    ('T-004', 2, 'was blocked',   'blocked',   '', 4, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                    ('T-005', 2, 'was done',      'done',      '', 5, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message)
             VALUES ('task', 3, '2026-01-01T00:00:00Z', 'me', 'CREATE', 'add task');
         INSERT INTO links (task_id, label, uri, created_at)
             VALUES (1, 'chat session', 'hermes://open/abc', '2026-01-01T00:00:00Z');
         INSERT INTO attachments (task_id, filename, stored_path, created_at)
             VALUES (3, 'fig.png', '.lun/attachments/fig.png', '2026-01-01T00:00:00Z');
         INSERT INTO prs (pr_key, task_id, source_branch, target_branch, created_at)
             VALUES ('PR-001', 3, 'fix-x', 'main', '2026-01-01T00:00:00Z');",
    )
    .unwrap();
}

fn raw(root: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap()
}

#[test]
fn status_set_is_the_obsidian_stages_in_board_order() {
    assert_eq!(
        TASK_STATUSES,
        ["planning", "review", "waiting-on", "backlog", "closed"]
    );
    assert_eq!(lun::db::CURRENT_VERSION, 9);
}

#[test]
fn v8_database_upgrades_to_v9_mapping_statuses_and_keeping_children() {
    let root = temp_root("v9-upgrade");
    make_v8_db(&root);

    let lun = Lun::init(&root).unwrap();
    assert_eq!(lun.schema_version().unwrap(), 9);

    let conn = raw(&root);
    let statuses: Vec<(String, String)> = conn
        .prepare("SELECT task_key, status FROM tasks ORDER BY task_key")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let want = [
        ("T-001", "planning"),
        ("T-002", "planning"),
        ("T-003", "review"),
        ("T-004", "waiting-on"),
        ("T-005", "closed"),
    ];
    assert_eq!(
        statuses,
        want.iter()
            .map(|(k, s)| (k.to_string(), s.to_string()))
            .collect::<Vec<_>>()
    );

    // Notes, positions and ids survive the table rebuild.
    let (notes, position): (String, i64) = conn
        .query_row(
            "SELECT notes, position FROM tasks WHERE task_key = 'T-001'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(notes, "# agent-status\n\nscoping");
    assert_eq!(position, 1);

    // Child rows still point at the same task ids, and their schemas still
    // reference `tasks` (not the temporary rebuild table).
    let link_task: i64 = conn
        .query_row("SELECT task_id FROM links WHERE label = 'chat session'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(link_task, 1);
    let pr_task: i64 = conn
        .query_row("SELECT task_id FROM prs WHERE pr_key = 'PR-001'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pr_task, 3);
    for table in ["links", "attachments", "prs"] {
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(sql.contains("REFERENCES tasks(id)"), "{table}: {sql}");
        assert!(!sql.contains("tasks_v9"), "{table}: {sql}");
    }
    let tmp_left: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'tasks_v9'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tmp_left, 0);
    let fk_violations: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk_violations, 0);

    // The new CHECK rejects the old names at the storage layer.
    let old = conn.execute(
        "INSERT INTO tasks (task_key, project_id, title, status, created_at, updated_at)
         VALUES ('T-099', 2, 'x', 'todo', 'now', 'now')",
        [],
    );
    assert!(old.is_err(), "old status must violate the v9 CHECK");

    // Re-opening is a no-op (idempotent migrations).
    drop(lun);
    let lun = Lun::open(&root).unwrap();
    assert_eq!(lun.schema_version().unwrap(), 9);
    let v9_rows: i64 = raw(&root)
        .query_row("SELECT COUNT(*) FROM migrations WHERE version = 9", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v9_rows, 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn old_status_names_are_accepted_as_aliases_and_bad_ones_list_the_stages() {
    let root = temp_root("v9-aliases");
    let lun = Lun::init(&root).unwrap();
    let mk = |title: &str, status: &str| {
        lun.create_task(TaskSpec {
            title: title.into(),
            status: Some(status.into()),
            ..Default::default()
        })
    };
    assert_eq!(mk("a", "todo").unwrap().status, "planning");
    assert_eq!(mk("b", "doing").unwrap().status, "planning");
    assert_eq!(mk("c", "follow-up").unwrap().status, "review");
    assert_eq!(mk("d", "blocked").unwrap().status, "waiting-on");
    assert_eq!(mk("e", "done").unwrap().status, "closed");
    assert_eq!(mk("f", "backlog").unwrap().status, "backlog");
    let defaulted = lun
        .create_task(TaskSpec {
            title: "g".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(defaulted.status, "planning");

    let err = mk("h", "in-review").unwrap_err().to_string();
    assert!(
        err.contains("planning, review, waiting-on, backlog, or closed"),
        "{err}"
    );

    let t = lun
        .update_task(
            defaulted.id,
            TaskUpdateSpec {
                status: Some("blocked".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(t.status, "waiting-on");

    // Filters accept aliases too, and the board sorts in stage order.
    let spec = |status: Option<&str>, sort: TaskSort| TaskListSpec {
        project_id: None,
        status: status.map(str::to_string),
        priority: None,
        assignee: None,
        scope: None,
        include_archived: false,
        sort,
    };
    let review: Vec<String> = lun
        .list_tasks_with(&spec(Some("follow-up"), TaskSort::Key))
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(review, ["c"]);
    let order: Vec<String> = lun
        .list_tasks_with(&spec(None, TaskSort::Status))
        .unwrap()
        .into_iter()
        .map(|t| t.status)
        .collect();
    let mut sorted = order.clone();
    sorted.sort_by_key(|s| TASK_STATUSES.iter().position(|x| x == s).unwrap());
    assert_eq!(order, sorted);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_status_flag_accepts_aliases_and_complete_closes() {
    let root = temp_root("v9-cli");
    let bin = env!("CARGO_BIN_EXE_lun");
    let run = |args: &[&str]| {
        let out = Command::new(bin)
            .args(args)
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    assert!(run(&["init"]).0);
    let (ok, out, err) = run(&["add", "task", "stage test"]);
    assert!(ok, "{out}{err}");
    let (ok, out, _) = run(&["task", "T-001", "--status", "follow-up", "--message", "x"]);
    assert!(ok);
    assert!(out.contains("status -> review"), "{out}");
    let (ok, out, _) = run(&["task", "T-001", "--status", "waiting-on", "--message", "x"]);
    assert!(ok);
    assert!(out.contains("status -> waiting-on"), "{out}");
    let (ok, _, err) = run(&["task", "T-001", "--status", "doing-it", "--message", "x"]);
    assert!(!ok);
    assert!(err.contains("planning, review, waiting-on, backlog, or closed"), "{err}");
    let (ok, _, _) = run(&["task", "complete", "T-001"]);
    assert!(ok);
    let (_, out, _) = run(&["task", "T-001"]);
    assert!(out.contains("Status:    closed"), "{out}");
    let (_, out, _) = run(&["status"]);
    assert!(
        out.contains("Planning   Review   Waiting-On   Backlog   Closed"),
        "{out}"
    );
    let (_, out, _) = run(&["status", "--json"]);
    assert!(out.contains("\"closed\": 1"), "{out}");
    assert!(out.contains("\"waiting-on\": 0"), "{out}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn log_entity_limit_returns_only_the_newest_entries() {
    let root = temp_root("v9-log-limit");
    let bin = env!("CARGO_BIN_EXE_lun");
    let run = |args: &[&str]| {
        let out = Command::new(bin)
            .args(args)
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    assert!(run(&["init"]).0);
    assert!(run(&["add", "task", "history"]).0);
    assert!(run(&["add", "task", "other"]).0);
    for m in ["first note", "second note", "third note"] {
        assert!(run(&["log", "T-001", "-m", m]).0);
    }
    assert!(run(&["log", "T-002", "-m", "not mine"]).0);

    // Entity + -N, either order: only that task's N newest entries.
    for args in [["log", "T-001", "-2", "--oneline"], ["log", "-2", "T-001", "--oneline"]] {
        let (ok, out, err) = run(&args);
        assert!(ok, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].contains("third note"), "{out}");
        assert!(lines[1].contains("second note"), "{out}");
        assert!(!out.contains("not mine"), "{out}");
    }
    // Full history of T-001 is CREATE + 3 commits.
    let (_, out, _) = run(&["log", "T-001", "--oneline"]);
    assert_eq!(out.lines().count(), 4, "{out}");
    // -0 and oversized counts are graceful.
    let (ok, out, _) = run(&["log", "T-001", "-0"]);
    assert!(ok);
    assert_eq!(out.trim(), "No logs found.");
    let (ok, out, _) = run(&["log", "T-001", "-100", "--oneline"]);
    assert!(ok);
    assert_eq!(out.lines().count(), 4, "{out}");
    // Global -N still works.
    let (ok, out, _) = run(&["log", "-1", "--oneline"]);
    assert!(ok);
    assert!(out.contains("not mine"), "{out}");
    // Unknown entity with a count is still an error, not a panic.
    let (ok, _, err) = run(&["log", "T-999", "-2"]);
    assert!(!ok);
    assert!(err.contains("not-found") || err.contains("no task"), "{err}");

    let _ = std::fs::remove_dir_all(&root);
}
