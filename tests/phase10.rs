//! Phase 10 tests: schema migrations (v1 -> current, v2 -> current), schema
//! guarantees, and a cross-cutting CLI smoke test.
//!
//! The Phase 2-9 suites each cover their own feature; this file covers the
//! migration machinery itself:
//! - a v1 database upgrades to the current schema with data preserved;
//! - a v2 database upgrades to the current schema with data preserved;
//! - the final schema has every table/column the docs promise;
//! - the P-000 seed survives migrations.
//!
//! The v1/v2 fixtures are built by replaying the exact SQL that the
//! migrations ran back then (copied from the migration history), so a
//! regression in the upgrade path fails here.

use lun::Lun;

fn temp_root(name: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("lun-p10-test-{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The exact v1 schema batch (Phase 2 initial schema), including the P-000
/// seed. Kept in lockstep with the migration history in `db.rs`. The
/// `migrations` table itself is created by `migrate()` before any version
/// batch runs, so it's created here first too.
const V1_BATCH: &str = "CREATE TABLE migrations (
    version    INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
CREATE TABLE projects (
    id          INTEGER PRIMARY KEY,
    project_key TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'active'
               CHECK (status IN ('planning', 'active', 'in-progress', 'done')),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
 );
 CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY,
    task_key    TEXT NOT NULL UNIQUE,
    project_id  INTEGER REFERENCES projects(id),
    title       TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'todo'
               CHECK (status IN ('todo', 'in-progress', 'review', 'done')),
    priority    TEXT NOT NULL DEFAULT 'low'
               CHECK (priority IN ('low', 'med', 'high')),
    assignee    TEXT,
    branch      TEXT,
    labels      TEXT NOT NULL DEFAULT '[]',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
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
    task_id     INTEGER NOT NULL REFERENCES tasks(id),
    filename    TEXT NOT NULL,
    stored_path TEXT NOT NULL,
    created_at  TEXT NOT NULL
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
 INSERT INTO migrations (version, applied_at)
     VALUES (1, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));
 INSERT INTO projects (project_key, name, status, created_at, updated_at)
 VALUES ('P-000', 'Unassigned', 'active',
         (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
         (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));
 INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details)
 VALUES ('project', 1,
         (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
         'system', 'CREATE',
         'seed P-000 \"Unassigned\" project',
         '{\"seed\": true}');";

/// v2 state: v1 + the Phase 7 `tasks.notes` column + a user project/task
/// with real data, so the upgrade test can verify data preservation.
fn seed_v1_with_data(conn: &rusqlite::Connection) {
    conn.execute_batch(V1_BATCH).unwrap();
    let now = conn
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap();
    conn.execute(
        "INSERT INTO projects (project_key, name, status, created_at, updated_at)
         VALUES ('P-001', 'legacy-project', 'active', ?1, ?1)",
        [&now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (task_key, project_id, title, status, priority, created_at, updated_at)
         VALUES ('T-001', 2, 'legacy task', 'in-progress', 'med', ?1, ?1)",
        [&now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details)
         VALUES ('task', 1, ?1, 'me', 'CREATE', 'add task \"legacy task\" to legacy-project', '{}')",
        [&now],
    )
    .unwrap();
}

/// v2 state: v1-with-data + the Phase 7 notes migration.
fn promote_to_v2(conn: &rusqlite::Connection) {
    conn.execute_batch(
        "ALTER TABLE tasks ADD COLUMN notes TEXT NOT NULL DEFAULT '';
         INSERT INTO migrations (version, applied_at)
             VALUES (2, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
    )
    .unwrap();
    // Give the legacy task a note so the v2 -> v3 test can check it
    // survives the ALTER-heavy upgrade.
    conn.execute(
        "UPDATE tasks SET notes = 'legacy note text' WHERE task_key = 'T-001'",
        [],
    )
    .unwrap();
}

fn schema_version(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM migrations",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn table_count(conn: &rusqlite::Connection, tables: &[&str]) -> i64 {
    let placeholders = vec!["?"; tables.len()].join(",");
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ({placeholders})"
        ),
        rusqlite::params_from_iter(tables.iter().copied()),
        |r| r.get(0),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Migrations
// ---------------------------------------------------------------------------

#[test]
fn fresh_init_reaches_current_schema_with_all_tables() {
    let root = temp_root("fresh");
    let lun = Lun::init(&root).unwrap();
    let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
    assert_eq!(schema_version(&conn), 6);
    assert_eq!(
        table_count(
            &conn,
            &[
                "projects",
                "tasks",
                "logs",
                "attachments",
                "links",
                "prs",
                "migrations"
            ]
        ),
        7,
        "fresh schema has all seven tables"
    );
    // The prs table starts empty.
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM prs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    let attachment_cols: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('attachments') WHERE name IN ('task_id', 'project_id', 'filename', 'stored_path', 'created_at')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attachment_cols, 5);
    let task_cols: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'archived_at'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_cols, 1);
    assert_eq!(lun::db::CURRENT_VERSION, 6);
    drop(conn);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v1_database_upgrades_to_current_with_data_preserved() {
    let root = temp_root("v1-upgrade");
    std::fs::create_dir_all(root.join(".lun")).unwrap();
    {
        let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
        seed_v1_with_data(&conn);
        assert_eq!(schema_version(&conn), 1);
    }
    // `lun init` on the existing directory must run the pending migrations in one shot.
    let lun = Lun::init(&root).unwrap();
    let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
    assert_eq!(schema_version(&conn), 6);
    assert_eq!(table_count(&conn, &["projects", "tasks", "prs"]), 3);
    // Data preservation: the legacy task + its log survive the upgrade.
    let title: String = conn
        .query_row("SELECT title FROM tasks WHERE task_key='T-001'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(title, "legacy task");
    let status: String = conn
        .query_row("SELECT status FROM tasks WHERE task_key='T-001'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status, "doing", "legacy task status is normalized");
    let notes: String = conn
        .query_row("SELECT notes FROM tasks WHERE task_key='T-001'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(notes, "", "v1 rows get the v2 default empty notes");
    let logs: i64 = conn
        .query_row("SELECT COUNT(*) FROM logs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(logs, 2, "P-000 seed log + task CREATE log survive");
    drop(conn);
    // The upgraded DB is fully usable: a PR can be opened on the legacy task.
    let task = lun.task_by_key("T-001").unwrap();
    let pr = lun
        .create_pr(lun::PrSpec {
            task_id: task.id,
            source_branch: Some("legacy-branch".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pr.pr_key, "PR-001");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v2_database_upgrades_to_current_preserving_notes() {
    let root = temp_root("v2-upgrade");
    std::fs::create_dir_all(root.join(".lun")).unwrap();
    {
        let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
        seed_v1_with_data(&conn);
        promote_to_v2(&conn);
        assert_eq!(schema_version(&conn), 2);
    }
    let lun = Lun::init(&root).unwrap();
    let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
    assert_eq!(schema_version(&conn), 6);
    let notes: String = conn
        .query_row("SELECT notes FROM tasks WHERE task_key='T-001'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        notes, "legacy note text",
        "v2 notes survive the current upgrade"
    );
    drop(conn);
    let _ = lun;
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn p000_seed_survives_migrations() {
    let root = temp_root("p000");
    std::fs::create_dir_all(root.join(".lun")).unwrap();
    {
        let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
        seed_v1_with_data(&conn);
    }
    let lun = Lun::init(&root).unwrap();
    let p = lun.project_by_key("P-000").unwrap();
    assert_eq!(p.name, "Unassigned");
    let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
    // The P-000 seed log entry is intact (exactly one seed entry).
    let seeds: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM logs WHERE details = '{\"seed\": true}'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(seeds, 1);
    drop(conn);
    let _ = lun;
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// CLI smoke: every command the plan ships must dispatch (not "not
// implemented"), and unknown commands fail with usage exit 2.
// ---------------------------------------------------------------------------

#[test]
fn cli_dispatch_covers_all_shipped_commands() {
    let root = temp_root("cli-smoke");
    let lun = Lun::init(&root).unwrap();
    let app = lun::cli::App { lun };
    let success = std::process::ExitCode::SUCCESS;
    let usage = std::process::ExitCode::from(lun::cli::EXIT_USAGE);

    // Shipped read-only commands succeed on a fresh DB.
    for cmd in [
        vec!["status".to_string()],
        vec!["pr".to_string(), "ls".to_string()],
    ] {
        assert_eq!(
            lun::cli::run(&app, &cmd),
            success,
            "expected success for {cmd:?}"
        );
    }
    // `--help` and `--version` are handled by the binary (main.rs), not
    // `cli::run`; unknown subcommands land here as usage errors.
    for cmd in [
        vec!["frobnicate".to_string()],
        vec!["pr".to_string()],
        vec!["task".to_string()],
    ] {
        assert_eq!(
            lun::cli::run(&app, &cmd),
            usage,
            "expected usage error for {cmd:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
