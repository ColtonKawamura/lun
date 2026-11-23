//! Phase 2: SQLite schema, migrations, and the data-access layer.
//!
//! Canonical storage is the DB (`.lun/lun.db`); markdown is a view, never a
//! source of truth. Every state-changing operation writes a `logs` entry
//! with a commit-style message (the "log-on-write" guarantee).
//!
//! Phase 9 adds the `prs` table (GitHub-style PRs over lun tasks) with
//! `create_pr` / `list_prs` / `pr_by_key` / `pr_by_task` / `merge_pr`,
//! all log-on-write (`CREATE`/`MERGE` on the PR itself plus a task entry
//! that maps the PR lifecycle onto the task's log).

use rusqlite::{params, types::Value, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const CURRENT_VERSION: i64 = 11;

/// Parent "hash" of the first log entry of an entity.
const GENESIS_LOG_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Task statuses, in board-column order (schema v9). These mirror Colton's
/// Obsidian kanban stages: `planning` (scope being worked out), `review`
/// (back with results / a PR / a decision), `waiting-on` (out with people or
/// an external process), `backlog`, and `closed`.
pub const TASK_STATUSES: [&str; 5] = ["planning", "review", "waiting-on", "backlog", "closed"];

/// Status a new task starts in.
pub const DEFAULT_TASK_STATUS: &str = "planning";

/// Status `complete`, `pr merge`, and the "done" shortcuts move a task to.
pub const CLOSED_TASK_STATUS: &str = "closed";

/// Map a user-typed status (including the pre-v9 names) onto a v9 status.
/// Unknown values pass through unchanged so validation can reject them.
pub fn normalize_task_status(status: &str) -> String {
    let status = status.trim().to_ascii_lowercase();
    match status.as_str() {
        "todo" | "doing" | "in-progress" | "open" => "planning".to_string(),
        "follow-up" | "followup" => "review".to_string(),
        "blocked" | "waiting" | "waiting-on" | "waitingon" => "waiting-on".to_string(),
        "done" | "complete" | "completed" => "closed".to_string(),
        _ => status,
    }
}

/// Is `status` one of the v9 task statuses (after normalization)?
pub fn is_task_status(status: &str) -> bool {
    TASK_STATUSES.contains(&status)
}

/// Human-readable list for error messages: "planning, review, ... or closed".
pub fn task_status_list() -> String {
    let (last, rest) = TASK_STATUSES.split_last().expect("non-empty");
    format!("{}, or {}", rest.join(", "), last)
}

/// Default actor for log entries (Phase 2 has no user-profile table yet).
const DEFAULT_USER: &str = "me";

#[derive(Debug)]
pub struct DbError {
    kind: &'static str,
    message: String,
}

impl DbError {
    pub(crate) fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Error category (`db`, `io`, `not-found`, `usage`, `ambiguous`, ...).
    pub fn kind(&self) -> &'static str {
        self.kind
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::new("db", e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Target a link record is attached to (a task or a project).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    Task(i64),
    Project(i64),
}

/// Target an attachment record is attached to (a task or a project).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentTarget {
    Task(i64),
    Project(i64),
}

/// Row of the `attachments` table (read model).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attachment {
    pub id: i64,
    pub task_id: Option<i64>,
    pub project_id: Option<i64>,
    pub filename: String,
    pub stored_path: String,
    pub created_at: String,
}

/// Row of the `links` table (read model).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    pub id: i64,
    pub task_id: Option<i64>,
    pub project_id: Option<i64>,
    pub label: String,
    pub uri: String,
    pub created_at: String,
}

/// Input for [`Lun::create_project`].
#[derive(Debug, Clone, Default)]
pub struct ProjectSpec {
    pub name: String,
    /// `active` or `inactive`. Defaults to `active`.
    pub status: Option<String>,
    /// Commit-style message for the log entry. Defaults to `create project "<name>"`.
    pub message: Option<String>,
    pub user: Option<String>,
}

/// Input for [`Lun::create_task`].
#[derive(Debug, Clone, Default)]
pub struct TaskSpec {
    pub title: String,
    /// Project the task belongs to; `None` falls back to P-000 "Unassigned".
    pub project: Option<i64>,
    /// `planning`, `review`, `waiting-on`, `backlog`, or `closed`. Defaults to `planning`.
    pub status: Option<String>,
    /// `low`, `med`, `high`. Defaults to `low`.
    pub priority: Option<String>,
    pub assignee: Option<String>,
    pub branch: Option<String>,
    /// Optional scope/workstream (e.g. "research", "personal"). Nullable.
    pub scope: Option<String>,
    /// JSON array of label strings, e.g. `["cli", "db"]`. Defaults to `[]`.
    pub labels: Option<String>,
    /// Commit-style message for the log entry. Defaults to `add task "<title>" to <project>`.
    pub message: Option<String>,
    pub user: Option<String>,
}

/// Row of the `prs` table (read model; Phase 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pr {
    pub id: i64,
    pub pr_key: String,
    pub task_id: i64,
    pub source_branch: String,
    pub target_branch: String,
    /// `open` | `merged`.
    pub status: String,
    pub created_at: String,
    pub merged_at: Option<String>,
}

/// Row of the `note_diffs` table (v10: incremental diffs of note edits).
///
/// Stores the unified diff (`diff`) and the *old* full notes text (`old_text`)
/// so that any previous version can be reconstructed without replaying all
/// diffs. The diff itself is compact — only the changed regions plus a few
/// context lines — so it is small relative to the full notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteDiff {
    pub id: i64,
    pub task_id: i64,
    /// Version number (1 = the first diff recorded for this task).
    pub version: i64,
    /// Unified diff from old_text -> new notes.
    pub diff: String,
    /// Full text of the notes before this change (for direct recovery).
    pub old_text: String,
    pub timestamp: String,
    pub user: String,
    /// Row id of the log entry that recorded this edit.
    pub log_id: Option<i64>,
    /// Hash of that log entry: `lun log <hash>` shows this diff.
    pub log_hash: Option<String>,
}

/// Input for [`Lun::create_pr`].
#[derive(Debug, Clone, Default)]
pub struct PrSpec {
    /// The task the PR is for (row id).
    pub task_id: i64,
    /// Branch being merged in. Defaults to the task's `branch` field.
    pub source_branch: Option<String>,
    /// Merge target. Defaults to `main`.
    pub target_branch: Option<String>,
    /// Commit-style message for the log entry. Defaults to
    /// `open PR-00N from <source> into <target> for <task>`.
    pub message: Option<String>,
    pub user: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: i64,
    pub timestamp: String,
    pub user: String,
    pub action: String,
    pub message: String,
    /// JSON object with field changes/notes (small, by design).
    pub details: String,
    /// SHA-256 hash (64-char hex) — like a git commit hash for the entry.
    pub hash: String,
}

/// Result of `delete_task`.
#[derive(Debug, Clone)]
pub struct TaskDeleted {
    pub task: Task,
    pub log_entries: usize,
    pub links: usize,
    pub attachments: usize,
    pub prs: usize,
    /// Stored paths of the removed attachment rows (the caller may delete
    /// managed copies from disk).
    pub attachment_paths: Vec<String>,
}

/// What `delete_project` does with a project's tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectDeleteMode {
    /// Refuse when the project still has tasks (the default).
    RefuseIfNotEmpty,
    /// Move every task to the project with this row id, then delete.
    MoveTo(i64),
    /// Delete the tasks (and their logs, links, attachments, PRs) too.
    Cascade,
}

/// Result of `delete_project`.
#[derive(Debug, Clone)]
pub struct ProjectDeleted {
    pub project: Project,
    pub tasks_moved: usize,
    pub tasks_deleted: usize,
    pub moved_to: Option<Project>,
    /// Stored paths of attachments whose rows were removed (the caller
    /// may delete managed copies from disk).
    pub attachment_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    pub id: i64,
    pub project_key: String,
    pub name: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    pub id: i64,
    pub task_key: String,
    pub project_id: Option<i64>,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub assignee: Option<String>,
    pub branch: Option<String>,
    /// Optional scope/workstream (e.g. "research", "personal"). Nullable.
    pub scope: Option<String>,
    /// JSON array of strings.
    pub labels: String,
    /// Free-form markdown notes (Phase 7: editable in the TUI).
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskSort {
    #[default]
    Key,
    Title,
    Status,
    Priority,
    Updated,
    /// Board order: by status column, then explicit within-column position.
    Board,
}

#[derive(Debug, Clone, Default)]
pub struct TaskListSpec {
    pub project_id: Option<i64>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<String>,
    pub scope: Option<String>,
    pub include_archived: bool,
    pub sort: TaskSort,
}

#[derive(Debug, Clone, Default)]
pub struct TaskUpdateSpec {
    pub title: Option<String>,
    pub project_id: Option<i64>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<Option<String>>,
    pub branch: Option<Option<String>>,
    pub scope: Option<Option<String>>,
    pub labels: Option<String>,
    pub notes: Option<String>,
    pub message: Option<String>,
    pub user: Option<String>,
}

/// Handle to an opened `.lun/lun.db`.
///
/// All public methods on this type either read or atomically mutate state;
/// every mutation is paired with a `logs` entry (log-on-write).
pub struct Lun {
    conn: Connection,
}

impl std::fmt::Debug for Lun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lun").finish_non_exhaustive()
    }
}

impl Lun {
    fn normalize_project_status(status: &str) -> String {
        match status {
            "inactive" | "done" => "inactive".to_string(),
            _ => "active".to_string(),
        }
    }

    fn normalize_task_status(status: &str) -> String {
        normalize_task_status(status)
    }

    fn validate_project_status(status: &str) -> Result<()> {
        if matches!(status, "active" | "inactive") {
            Ok(())
        } else {
            Err(DbError::new(
                "invalid",
                format!("invalid project status '{status}' (expected active or inactive)"),
            ))
        }
    }

    fn validate_task_status(status: &str) -> Result<()> {
        if is_task_status(status) {
            Ok(())
        } else {
            Err(DbError::new(
                "invalid",
                format!("invalid status '{status}' (expected {})", task_status_list()),
            ))
        }
    }

    /// Open (creating if needed) `.lun/lun.db` under `root`, ensure the
    /// directory exists, and run any pending migrations.
    ///
    /// This is the backing operation for `lun init`; it is idempotent —
    /// re-running it on an initialized directory is a no-op.
    pub fn init(root: &Path) -> Result<Self> {
        let lun_dir = root.join(".lun");
        std::fs::create_dir_all(&lun_dir)
            .map_err(|e| DbError::new("io", format!("creating {}: {}", lun_dir.display(), e)))?;

        let mut conn = Connection::open(lun_dir.join("lun.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Open an existing, already-initialized DB (no directory creation, no
    /// schema guarantees beyond what the file already has).
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join(".lun/lun.db");
        if !path.exists() {
            return Err(DbError::new(
                "not-initialized",
                format!("{} does not exist; run `lun init` first", path.display()),
            ));
        }
        let mut conn = Connection::open(&path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM migrations",
                [],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }

    /// Execute a PRAGMA statement (e.g. busy_timeout, journal_mode).
    pub fn set_pragma(&self, pragma: &str) -> Result<()> {
        self.conn
            .execute_batch(pragma)
            .map_err(|e| DbError::new("pragma", format!("{pragma}: {e}")).into())
    }

    fn bootstrap_current_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE projects (
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
                status      TEXT NOT NULL DEFAULT 'planning'
                           CHECK (status IN ('planning', 'review', 'waiting-on', 'backlog', 'closed')),
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
                details     TEXT NOT NULL DEFAULT '{}',
                hash        TEXT NOT NULL DEFAULT ''
             );
             CREATE INDEX idx_logs_hash ON logs(hash);
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
             CREATE TABLE note_diffs (
                id          INTEGER PRIMARY KEY,
                task_id     INTEGER NOT NULL REFERENCES tasks(id),
                version     INTEGER NOT NULL,
                diff        TEXT NOT NULL,
                old_text    TEXT NOT NULL,
                timestamp   TEXT NOT NULL,
                user        TEXT NOT NULL,
                log_id      INTEGER REFERENCES logs(id)
             );
             INSERT INTO projects (project_key, name, status, created_at, updated_at)
             VALUES ('P-000', 'Unassigned', 'active',
                     (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));
             INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details)
             VALUES ('project', 1,
                     (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     'system', 'CREATE',
                     'seed P-000 \"Unassigned\" project',
                     '{\"seed\": true}');",
        )?;
        // Record the *current* schema version (not a hardcoded number, which
        // is how v10 shipped a fresh-board bug) and hash the seed entry.
        conn.execute(
            "INSERT INTO migrations (version, applied_at)
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            [CURRENT_VERSION],
        )?;
        Self::backfill_log_hashes(conn)?;
        Ok(())
    }

    fn migrate(conn: &mut Connection) -> Result<()> {
        let user_tables_before: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master
              WHERE type = 'table'
                AND name != 'sqlite_sequence'",
            [],
            |r| r.get(0),
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS migrations (
                version    INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
             );",
        )?;

        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM migrations",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if version == 0 && user_tables_before == 0 {
            Self::bootstrap_current_schema(conn)?;
        } else {
            if version < 1 {
                conn.execute_batch(
                    "CREATE TABLE projects (
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
                 -- P-000 \"Unassigned\": every task without an explicit
                 -- project lands here. It is seeded, not user-created.
                 INSERT INTO projects (project_key, name, status, created_at, updated_at)
                 VALUES ('P-000', 'Unassigned', 'active',
                         (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                         (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));
                 INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details)
                 VALUES ('project', 1,
                         (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                         'system', 'CREATE',
                         'seed P-000 \"Unassigned\" project',
                         '{\"seed\": true}');",
                )?;
            }

            if version < 2 {
                // Phase 7: tasks gain a `notes` column (free-form markdown,
                // editable in the TUI; saved notes log UPDATE).
                conn.execute_batch(
                    "ALTER TABLE tasks ADD COLUMN notes TEXT NOT NULL DEFAULT '';
                     INSERT INTO migrations (version, applied_at)
                         VALUES (2, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
                )?;
            }

            if version < 3 {
                // Phase 9: the `prs` table — lun's GitHub-style PRs. A PR is
                // always about one task (task_id FK); source/target branches
                // are recorded for the `git diff`/`git merge` glue.
                conn.execute_batch(
                    "CREATE TABLE prs (
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
                     INSERT INTO migrations (version, applied_at)
                         VALUES (3, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
                )?;
            }

            if version < 4 {
                conn.execute_batch(
                    "ALTER TABLE attachments RENAME TO attachments_v1;
                    CREATE TABLE attachments (
                       id          INTEGER PRIMARY KEY,
                       task_id     INTEGER REFERENCES tasks(id),
                       project_id  INTEGER REFERENCES projects(id),
                       filename    TEXT NOT NULL,
                       stored_path TEXT NOT NULL,
                       created_at  TEXT NOT NULL,
                       CHECK ((task_id IS NULL) <> (project_id IS NULL))
                    );
                    INSERT INTO attachments (id, task_id, project_id, filename, stored_path, created_at)
                    SELECT id, task_id, NULL, filename, stored_path, created_at
                      FROM attachments_v1;
                    DROP TABLE attachments_v1;
                    INSERT INTO migrations (version, applied_at)
                        VALUES (4, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
                )?;
            }

            if version < 5 {
                conn.execute_batch(
                    "ALTER TABLE tasks ADD COLUMN archived_at TEXT;
                    INSERT INTO migrations (version, applied_at)
                        VALUES (5, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
                )?;
            }

            if version < 6 {
                let table_exists = |name: &str| -> Result<bool> {
                    let exists: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                        [name],
                        |r| r.get(0),
                    )?;
                    Ok(exists > 0)
                };
                let has_attachments = table_exists("attachments")?;
                let has_links = table_exists("links")?;
                let has_prs = table_exists("prs")?;

                let mut sql = String::new();
                if has_attachments {
                    sql.push_str("ALTER TABLE attachments RENAME TO attachments_v5;");
                }
                if has_links {
                    sql.push_str("ALTER TABLE links RENAME TO links_v5;");
                }
                if has_prs {
                    sql.push_str("ALTER TABLE prs RENAME TO prs_v5;");
                }
                sql.push_str(
                    "ALTER TABLE projects RENAME TO projects_v5;
                ALTER TABLE tasks RENAME TO tasks_v5;
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
                   created_at  TEXT NOT NULL,
                   updated_at  TEXT NOT NULL,
                   archived_at TEXT
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
                INSERT INTO projects (id, project_key, name, status, created_at, updated_at)
                SELECT id,
                       project_key,
                       name,
                       CASE
                           WHEN status = 'inactive' THEN 'inactive'
                           WHEN status = 'done' THEN 'inactive'
                           ELSE 'active'
                       END,
                       created_at,
                       updated_at
                  FROM projects_v5;
                INSERT INTO tasks (id, task_key, project_id, title, status, priority, assignee, branch, labels, scope, notes, created_at, updated_at, archived_at)
                SELECT id,
                       task_key,
                       project_id,
                       title,
                       CASE
                           WHEN status = 'in-progress' THEN 'doing'
                           WHEN status = 'review' THEN 'follow-up'
                           ELSE status
                       END,
                       priority,
                       assignee,
                       branch,
                       labels,
                       NULL,
                       notes,
                       created_at,
                       updated_at,
                       archived_at
                  FROM tasks_v5;",
                );
                if has_attachments {
                    sql.push_str(
                       "INSERT INTO attachments (id, task_id, project_id, filename, stored_path, created_at)
                    SELECT id, task_id, project_id, filename, stored_path, created_at
                      FROM attachments_v5;
                    DROP TABLE attachments_v5;",
                    );
                }
                if has_links {
                    sql.push_str(
                        "INSERT INTO links (id, task_id, project_id, label, uri, created_at)
                    SELECT id, task_id, project_id, label, uri, created_at
                      FROM links_v5;
                    DROP TABLE links_v5;",
                    );
                }
                if has_prs {
                    sql.push_str(
                       "INSERT INTO prs (id, pr_key, task_id, source_branch, target_branch, status, created_at, merged_at)
                    SELECT id, pr_key, task_id, source_branch, target_branch, status, created_at, merged_at
                      FROM prs_v5;
                    DROP TABLE prs_v5;",
                    );
                }
                sql.push_str(
                    "DROP TABLE tasks_v5;
                    DROP TABLE projects_v5;
                    INSERT INTO migrations (version, applied_at)
                       VALUES (6, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
                );
                conn.execute_batch(&sql)?;
            }

            if version < 7 {
                // Phase 10: tasks gain a `scope` column — an optional
                // workstream/area tag (e.g. "research", "personal", "home").
                // Only add if not already present (v6 migration may have added it).
                let scope_exists: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'scope'",
                    [],
                    |r| r.get(0),
                )?;
                if scope_exists == 0 {
                    conn.execute("ALTER TABLE tasks ADD COLUMN scope TEXT", [])?;
                }
                conn.execute(
                    "INSERT INTO migrations (version, applied_at)
                     VALUES (7, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')))",
                    [],
                )?;
            }

            if version < 8 {
                // Phase 11: explicit within-column ordering. `position` lets
                // the board keep a stable per-column order independent of
                // task_key, so cards can be reordered (<Space>j/k).
                let pos_exists: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'position'",
                    [],
                    |r| r.get(0),
                )?;
                if pos_exists == 0 {
                    conn.execute("ALTER TABLE tasks ADD COLUMN position INTEGER", [])?;
                    // Seed: preserve the current display order (task_key rank).
                    conn.execute(
                        "UPDATE tasks SET position = (
                             SELECT COUNT(*) + 1 FROM tasks t2 WHERE t2.task_key < tasks.task_key
                         )",
                        [],
                    )?;
                }
                conn.execute(
                    "INSERT INTO migrations (version, applied_at)
                     VALUES (8, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')))",
                    [],
                )?;
            }

            if version < 9 {
                Self::migrate_v9_obsidian_stages(conn)?;
            }

            if version < 10 {
                Self::migrate_v10_note_diffs(conn)?;
            }

            if version < 11 {
                Self::migrate_v11_log_hashes(conn)?;
            }
        }

        let version: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM migrations",
            [],
            |r| r.get(0),
        )?;
        if version != CURRENT_VERSION {
            return Err(DbError::new(
                "version-mismatch",
                format!("database is at schema version {version}, lun expects {CURRENT_VERSION}"),
            ));
        }
        Ok(())
    }

    /// v9: task statuses become Colton's Obsidian kanban stages
    /// (`planning`, `review`, `waiting-on`, `backlog`, `closed`).
    ///
    /// SQLite cannot alter a CHECK constraint, so `tasks` is rebuilt with the
    /// documented 12-step procedure: foreign keys off, copy into a new table
    /// with the new CHECK, drop the old one, rename, check FKs, foreign keys
    /// back on. Old values map `todo`/`doing` -> `planning`,
    /// `follow-up` -> `review`, `blocked` -> `waiting-on`, `done` -> `closed`.
    fn migrate_v9_obsidian_stages(conn: &mut Connection) -> Result<()> {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        let result = (|| -> Result<()> {
            let tx = conn.transaction()?;
            tx.execute_batch(
                "CREATE TABLE tasks_v9 (
                    id          INTEGER PRIMARY KEY,
                    task_key    TEXT NOT NULL UNIQUE,
                    project_id  INTEGER REFERENCES projects(id),
                    title       TEXT NOT NULL,
                    status      TEXT NOT NULL DEFAULT 'planning'
                               CHECK (status IN ('planning', 'review', 'waiting-on', 'backlog', 'closed')),
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
                 INSERT INTO tasks_v9 (id, task_key, project_id, title, status, priority,
                                       assignee, branch, labels, scope, notes, position,
                                       created_at, updated_at, archived_at)
                 SELECT id, task_key, project_id, title,
                        CASE status
                            WHEN 'todo'      THEN 'planning'
                            WHEN 'doing'     THEN 'planning'
                            WHEN 'follow-up' THEN 'review'
                            WHEN 'blocked'   THEN 'waiting-on'
                            WHEN 'done'      THEN 'closed'
                            ELSE status
                        END,
                        priority, assignee, branch, labels, scope, notes, position,
                        created_at, updated_at, archived_at
                   FROM tasks;
                 DROP TABLE tasks;
                 ALTER TABLE tasks_v9 RENAME TO tasks;
                 INSERT INTO migrations (version, applied_at)
                     VALUES (9, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
            )?;
            let fk_violations: i64 =
                tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
            if fk_violations != 0 {
                return Err(DbError::new(
                    "migration",
                    format!("v9: {fk_violations} foreign-key violations after rebuilding tasks"),
                ));
            }
            tx.commit()?;
            Ok(())
        })();
        conn.pragma_update(None, "foreign_keys", "ON")?;
        result
    }

    /// v10: `note_diffs` table — incremental diffs of note edits for
    /// recoverable history. Each diff records the unified diff plus the old
    /// notes text so any previous version can be recovered without replaying
    /// the full diff chain.
    fn migrate_v10_note_diffs(conn: &mut Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS note_diffs (
                id          INTEGER PRIMARY KEY,
                task_id     INTEGER NOT NULL REFERENCES tasks(id),
                version     INTEGER NOT NULL,
                diff        TEXT NOT NULL,
                old_text    TEXT NOT NULL,
                timestamp   TEXT NOT NULL,
                user        TEXT NOT NULL
             );
             INSERT INTO migrations (version, applied_at)
                 VALUES (10, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
        )?;
        Ok(())
    }

    /// v11: every log entry gets a git-style hash, chained to the previous
    /// entry of the same entity, and each note diff remembers the log entry
    /// that recorded it. Existing rows are backfilled in `id` order.
    fn migrate_v11_log_hashes(conn: &mut Connection) -> Result<()> {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "ALTER TABLE logs ADD COLUMN hash TEXT NOT NULL DEFAULT '';
             ALTER TABLE note_diffs ADD COLUMN log_id INTEGER REFERENCES logs(id);
             CREATE INDEX IF NOT EXISTS idx_logs_hash ON logs(hash);",
        )?;
        Self::backfill_log_hashes(&tx)?;
        Self::link_note_diffs_to_logs(&tx)?;
        tx.execute_batch(
            "INSERT INTO migrations (version, applied_at)
                 VALUES (11, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Best effort for diffs recorded under v10, which did not remember their
    /// log entry. Every diff was written together with exactly one `UPDATE`
    /// entry whose changes mention `notes:`, so per task the diffs (by
    /// version) pair up with those entries (by id), aligned from the newest
    /// end: older notes edits predate v10, have an entry but no diff.
    /// (Timestamps cannot do this job: they only have one-second resolution.)
    /// A task whose entries cannot be aligned keeps `log_id = NULL`.
    fn link_note_diffs_to_logs(conn: &Connection) -> Result<()> {
        let task_ids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT DISTINCT task_id FROM note_diffs")?;
            let ids = stmt
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<Vec<i64>, _>>()?;
            ids
        };
        for task_id in task_ids {
            let diffs: Vec<(i64, String)> = {
                let mut stmt = conn.prepare(
                    "SELECT id, timestamp FROM note_diffs WHERE task_id = ?1 ORDER BY version",
                )?;
                let rows = stmt
                    .query_map([task_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                rows
            };
            let logs: Vec<(i64, String)> = {
                let mut stmt = conn.prepare(
                    "SELECT id, timestamp FROM logs
                      WHERE entity_type = 'task' AND entity_id = ?1 AND action = 'UPDATE'
                        AND details LIKE '%notes: %'
                      ORDER BY id",
                )?;
                let rows = stmt
                    .query_map([task_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                rows
            };
            if logs.len() < diffs.len() {
                continue;
            }
            let offset = logs.len() - diffs.len();
            for (i, (diff_id, diff_ts)) in diffs.iter().enumerate() {
                let (log_id, log_ts) = &logs[offset + i];
                // The entry is written right after its diff, never before.
                if log_ts >= diff_ts {
                    conn.execute(
                        "UPDATE note_diffs SET log_id = ?1 WHERE id = ?2",
                        params![log_id, diff_id],
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Hash of one log entry: SHA-256 over the parent hash plus every field
    /// of the entry (NUL-separated, so a `|` in a message cannot collide).
    /// Chaining to the parent makes two otherwise identical entries (same
    /// message in the same second) hash differently, like git commits.
    #[allow(clippy::too_many_arguments)]
    fn compute_log_hash(
        parent_hash: &str,
        entity_type: &str,
        entity_id: i64,
        timestamp: &str,
        user: &str,
        action: &str,
        message: &str,
        details: &str,
    ) -> String {
        let mut hasher = Sha256::new();
        let id = entity_id.to_string();
        for part in [
            parent_hash, entity_type, id.as_str(), timestamp, user, action, message, details,
        ] {
            hasher.update(part.as_bytes());
            hasher.update([0u8]);
        }
        format!("{:x}", hasher.finalize())
    }

    /// Hash of the newest entry of this entity (the "parent" of the next
    /// one), or the all-zero genesis hash when it has none yet.
    fn parent_log_hash(&self, entity_type: &str, entity_id: i64) -> Result<String> {
        let parent: Option<String> = self
            .conn
            .query_row(
                "SELECT hash FROM logs WHERE entity_type = ?1 AND entity_id = ?2
                 ORDER BY id DESC LIMIT 1",
                params![entity_type, entity_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(parent.unwrap_or_else(|| GENESIS_LOG_HASH.to_string()))
    }

    /// Fill in `hash` for every log row that has none, oldest first, chaining
    /// each entry to the previous entry of the same entity. Idempotent. Runs
    /// no transaction of its own: callers (the v11 migration, bootstrap) own it.
    fn backfill_log_hashes(conn: &Connection) -> Result<usize> {
        let mut latest: std::collections::HashMap<(String, i64), String> =
            std::collections::HashMap::new();
        let mut pending: Vec<(i64, String)> = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT id, entity_type, entity_id, timestamp, user, action, message, details, hash
                 FROM logs ORDER BY id ASC",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                let id: i64 = r.get(0)?;
                let entity_type: String = r.get(1)?;
                let entity_id: i64 = r.get(2)?;
                let existing: String = r.get(8)?;
                let key = (entity_type, entity_id);
                if existing.is_empty() {
                    let parent = latest
                        .get(&key)
                        .map(String::as_str)
                        .unwrap_or(GENESIS_LOG_HASH);
                    let hash = Self::compute_log_hash(
                        parent,
                        &key.0,
                        key.1,
                        &r.get::<_, String>(3)?,
                        &r.get::<_, String>(4)?,
                        &r.get::<_, String>(5)?,
                        &r.get::<_, String>(6)?,
                        &r.get::<_, String>(7)?,
                    );
                    pending.push((id, hash.clone()));
                    latest.insert(key, hash);
                } else {
                    latest.insert(key, existing);
                }
            }
        }
        for (id, hash) in &pending {
            conn.execute("UPDATE logs SET hash = ?1 WHERE id = ?2", params![hash, id])?;
        }
        Ok(pending.len())
    }

    /// `T-013` / `P-003`: the key of the entity a log entry belongs to.
    pub fn entity_key(&self, entity_type: &str, entity_id: i64) -> Result<String> {
        let sql = if entity_type == "task" {
            "SELECT task_key FROM tasks WHERE id = ?1"
        } else {
            "SELECT project_key FROM projects WHERE id = ?1"
        };
        Ok(self.conn.query_row(sql, [entity_id], |r| r.get(0))?)
    }

    /// Look up one log entry by its hash or a prefix of it (4+ hex chars,
    /// like a short git SHA). Ambiguous prefixes are an error, not a guess.
    pub fn log_by_hash(&self, prefix: &str) -> Result<LogEntry> {
        let prefix = prefix.trim().to_ascii_lowercase();
        if prefix.len() < 4 || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(DbError::new(
                "usage",
                format!("'{prefix}' is not a log hash (need 4+ hex characters)"),
            ));
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, entity_type, entity_id, timestamp, user, action, message, details, hash
             FROM logs WHERE hash LIKE ?1 ORDER BY id ASC LIMIT 2",
        )?;
        let mut found = stmt
            .query_map([format!("{prefix}%")], Self::log_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        match found.len() {
            0 => Err(DbError::new(
                "not-found",
                format!("no log entry with hash '{prefix}'"),
            )),
            1 => Ok(found.remove(0)),
            _ => Err(DbError::new(
                "ambiguous",
                format!("hash prefix '{prefix}' matches more than one log entry; use more characters"),
            )),
        }
    }

    pub fn now() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format_epoch(d.as_secs()))
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
    }

    fn json_escape(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    }

    /// Generate a unified diff between two notes texts.
    ///
    /// Returns `None` if the texts are identical. Uses the `similar` crate
    /// (Myers diff) to produce a compact, human-readable diff that only
    /// includes changed regions plus a few context lines.
    fn compute_diff(old: &str, new: &str) -> Option<String> {
        if old == new {
            return None;
        }
        // Notes rarely end in a newline; diffing them as-is litters every hunk
        // with `\ No newline at end of file`. Pad both sides for the display
        // diff only (the full old text is stored untouched for recovery).
        let pad = |t: &str| -> String {
            if t.is_empty() || t.ends_with('\n') {
                t.to_string()
            } else {
                format!("{t}\n")
            }
        };
        let render = |a: &str, b: &str| -> String {
            let diff = similar::TextDiff::from_lines(a, b);
            let mut unified = diff.unified_diff();
            unified.context_radius(3).header("notes", "notes");
            unified.to_string()
        };
        let mut output = render(&pad(old), &pad(new));
        if output.is_empty() {
            // Only the trailing newline differs: show that honestly.
            output = render(old, new);
        }
        if output.is_empty() { None } else { Some(output) }
    }

    /// Save a note diff record when notes change on a task.
    ///
    /// `old_text` is the notes before the change.  `new_notes` is the notes
    /// after.  A unified diff is computed and stored alongside `old_text`
    /// so any previous version can be recovered without replaying the chain.
    /// `log_id` is the log entry that records this edit, so a log hash can be
    /// turned back into the diff.
    fn save_note_diff(
        &self,
        task_id: i64,
        old_text: &str,
        new_notes: &str,
        user: &str,
        log_id: i64,
    ) -> Result<()> {
        let diff = match Self::compute_diff(old_text, new_notes) {
            Some(d) => d,
            None => return Ok(()), // no change, skip
        };

        // Next version number for this task
        let version: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM note_diffs WHERE task_id = ?1",
            [task_id],
            |r| r.get(0),
        )?;

        self.conn.execute(
            "INSERT INTO note_diffs (task_id, version, diff, old_text, timestamp, user, log_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![task_id, version, diff, old_text, Self::now(), user, log_id],
        )?;
        Ok(())
    }

    const NOTE_DIFF_COLUMNS: &'static str =
        "d.id, d.task_id, d.version, d.diff, d.old_text, d.timestamp, d.user, d.log_id, l.hash";

    fn note_diff_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<NoteDiff> {
        Ok(NoteDiff {
            id: r.get(0)?,
            task_id: r.get(1)?,
            version: r.get(2)?,
            diff: r.get(3)?,
            old_text: r.get(4)?,
            timestamp: r.get(5)?,
            user: r.get(6)?,
            log_id: r.get(7)?,
            log_hash: r.get(8)?,
        })
    }

    /// Retrieve all note diffs for a task, oldest first.
    pub fn get_note_diffs(&self, task_id: i64) -> Result<Vec<NoteDiff>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM note_diffs d LEFT JOIN logs l ON l.id = d.log_id
             WHERE d.task_id = ?1 ORDER BY d.version ASC",
            Self::NOTE_DIFF_COLUMNS
        ))?;
        let diffs = stmt
            .query_map([task_id], Self::note_diff_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(diffs)
    }

    /// The note diff recorded by one log entry, if that entry edited notes.
    pub fn note_diff_for_log(&self, log_id: i64) -> Result<Option<NoteDiff>> {
        let diff = self
            .conn
            .query_row(
                &format!(
                    "SELECT {} FROM note_diffs d LEFT JOIN logs l ON l.id = d.log_id
                     WHERE d.log_id = ?1",
                    Self::NOTE_DIFF_COLUMNS
                ),
                [log_id],
                Self::note_diff_from_row,
            )
            .optional()?;
        Ok(diff)
    }

    // ------------------------------------------------------------------
    // Log entries
    // ------------------------------------------------------------------

    /// Record a log entry. `entity_id` is the row id of the `project_type`
    /// entity; callers use this directly only for records that are not
    /// themselves state changes (e.g. comments in a later phase).
    pub fn log(
        &self,
        entity_type: &str,
        entity_id: i64,
        action: &str,
        message: &str,
        details: &str,
        user: Option<&str>,
    ) -> Result<i64> {
        let user = user.unwrap_or(DEFAULT_USER);
        let timestamp = Self::now();
        let parent = self.parent_log_hash(entity_type, entity_id)?;
        let hash = Self::compute_log_hash(&parent, entity_type, entity_id, &timestamp, user, action, message, details);
        self.conn.execute(
            "INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entity_type,
                entity_id,
                timestamp,
                user,
                action,
                message,
                details,
                hash,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    // ------------------------------------------------------------------
    // Projects
    // ------------------------------------------------------------------

    /// Create a project. The key is the next free `P-00N` counter (P-000 is
    /// reserved for the seeded "Unassigned" project, so real projects start
    /// at P-001). Writes a `CREATE` log entry.
    pub fn create_project(&self, spec: ProjectSpec) -> Result<Project> {
        let user = spec.user.as_deref().unwrap_or(DEFAULT_USER);
        let status = Self::normalize_project_status(spec.status.as_deref().unwrap_or("active"));
        Self::validate_project_status(&status)?;
        let key = self.next_project_key()?;
        let now = Self::now();

        self.conn.execute(
            "INSERT INTO projects (project_key, name, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![key, spec.name, status, now, now],
        )?;
        let id = self.conn.last_insert_rowid();

        let message = spec
            .message
            .unwrap_or_else(|| format!("create project \"{}\"", spec.name));
        self.log(
            "project",
            id,
            "CREATE",
            &message,
            &format!("{{\"project_key\": \"{key}\", \"status\": \"{status}\"}}"),
            Some(user),
        )?;

        Ok(Project {
            id,
            project_key: key,
            name: spec.name,
            status,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn update_project_status(
        &self,
        project_id: i64,
        status: &str,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<Project> {
        let user = user.unwrap_or(DEFAULT_USER);
        let mut project = self
            .conn
            .query_row(
                "SELECT id, project_key, name, status, created_at, updated_at
                 FROM projects WHERE id = ?1",
                [project_id],
                Self::project_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("project {project_id}: {e}")))?;
        let target = Self::normalize_project_status(status);
        Self::validate_project_status(&target)?;
        if project.status == target {
            return Ok(project);
        }
        let now = Self::now();
        self.conn.execute(
            "UPDATE projects SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![target, now, project_id],
        )?;
        let changes = format!("status: {} -> {}", project.status, target);
        let message = message.map(str::to_string).unwrap_or_else(|| {
            format!(
                "update project {} status to {}",
                project.project_key, target
            )
        });
        self.log(
            "project",
            project_id,
            "UPDATE",
            &message,
            &format!("{{\"changes\": \"{}\"}}", Self::json_escape(&changes)),
            Some(user),
        )?;
        project.status = target;
        project.updated_at = now;
        Ok(project)
    }

    /// Permanently delete a task: the row, its log history, note diffs,
    /// links, attachment rows and PRs, in one transaction. This is not
    /// `archive_task` (which hides it and can be undone). The deletion is
    /// recorded as a `DELETE` entry on the task's project (or on P-000 when
    /// it has none), since the task's own history goes with it.
    pub fn delete_task(
        &self,
        task_id: i64,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<TaskDeleted> {
        let user = user.unwrap_or(DEFAULT_USER);
        let task = self.task_by_id(task_id)?;
        let project = match task.project_id {
            Some(pid) => self
                .conn
                .query_row(
                    "SELECT id, project_key, name, status, created_at, updated_at
                     FROM projects WHERE id = ?1",
                    [pid],
                    Self::project_from_row,
                )
                .ok(),
            None => None,
        };
        let log_target = match &project {
            Some(p) => p.id,
            None => self.unassigned_project_id()?,
        };

        let tx = self.conn.unchecked_transaction()?;
        let mut attachment_paths: Vec<String> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT stored_path FROM attachments WHERE task_id = ?1")?;
            let rows = stmt.query_map([task_id], |r| r.get::<_, String>(0))?;
            for r in rows {
                attachment_paths.push(r?);
            }
        }
        let count = |sql: &str| -> Result<usize> {
            let n: i64 = self.conn.query_row(sql, [task_id], |r| r.get(0))?;
            Ok(n as usize)
        };
        let links = count("SELECT COUNT(*) FROM links WHERE task_id = ?1")?;
        let prs = count("SELECT COUNT(*) FROM prs WHERE task_id = ?1")?;
        let log_entries = count(
            "SELECT COUNT(*) FROM logs WHERE entity_type = 'task' AND entity_id = ?1",
        )?;

        self.conn.execute("DELETE FROM attachments WHERE task_id = ?1", [task_id])?;
        self.conn.execute("DELETE FROM links WHERE task_id = ?1", [task_id])?;
        self.conn.execute("DELETE FROM prs WHERE task_id = ?1", [task_id])?;
        // note_diffs.log_id points at logs, so diffs go before the logs.
        self.conn.execute("DELETE FROM note_diffs WHERE task_id = ?1", [task_id])?;
        self.conn.execute(
            "DELETE FROM logs WHERE entity_type = 'task' AND entity_id = ?1",
            [task_id],
        )?;
        self.conn.execute("DELETE FROM tasks WHERE id = ?1", [task_id])?;

        let message = message
            .map(str::to_string)
            .unwrap_or_else(|| format!("delete task {} \"{}\"", task.task_key, task.title));
        self.log(
            "project",
            log_target,
            "DELETE",
            &message,
            &serde_json::json!({
                "task_key": task.task_key,
                "title": task.title,
                "status": task.status,
                "project_key": project.as_ref().map(|p| p.project_key.clone()),
                "log_entries": log_entries,
                "links": links,
                "attachments": attachment_paths.len(),
                "prs": prs,
            })
            .to_string(),
            Some(user),
        )?;
        tx.commit()?;
        Ok(TaskDeleted {
            task,
            log_entries,
            links,
            attachments: attachment_paths.len(),
            prs,
            attachment_paths,
        })
    }

    /// Number of tasks (archived included) that belong to a project.
    pub fn project_task_count(&self, project_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1",
            [project_id],
            |r| r.get(0),
        )?)
    }

    /// Delete a project. Refuses P-000 "Unassigned". A project that still
    /// has tasks is refused unless `how` says what to do with them: move
    /// them to another project, or delete them too. Everything happens in
    /// one transaction. The project's own history goes with it (row ids
    /// can be reused, so orphaned log rows would attach to a future
    /// project). Returns a summary of what was removed.
    pub fn delete_project(
        &self,
        project_id: i64,
        how: ProjectDeleteMode,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<ProjectDeleted> {
        let user = user.unwrap_or(DEFAULT_USER);
        let project = self
            .conn
            .query_row(
                "SELECT id, project_key, name, status, created_at, updated_at
                 FROM projects WHERE id = ?1",
                [project_id],
                Self::project_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("project {project_id}: {e}")))?;
        if project.project_key == "P-000" {
            return Err(DbError::new(
                "invalid",
                "P-000 Unassigned is the board's default project and cannot be deleted",
            ));
        }
        let tasks = self.tasks_for_project(project_id)?;
        let mut moved_to: Option<Project> = None;
        match &how {
            ProjectDeleteMode::RefuseIfNotEmpty if !tasks.is_empty() => {
                return Err(DbError::new(
                    "invalid",
                    format!(
                        "project {} [{}] still has {} task{}: pass --move-to <project> to move them, or --cascade to delete them too",
                        project.name,
                        project.project_key,
                        tasks.len(),
                        if tasks.len() == 1 { "" } else { "s" }
                    ),
                ));
            }
            ProjectDeleteMode::MoveTo(target_id) => {
                if *target_id == project_id {
                    return Err(DbError::new(
                        "invalid",
                        "--move-to must be a different project than the one being deleted",
                    ));
                }
                moved_to = Some(
                    self.conn
                        .query_row(
                            "SELECT id, project_key, name, status, created_at, updated_at
                             FROM projects WHERE id = ?1",
                            [target_id],
                            Self::project_from_row,
                        )
                        .map_err(|e| {
                            DbError::new("not-found", format!("project {target_id}: {e}"))
                        })?,
                );
            }
            _ => {}
        }

        let tx = self.conn.unchecked_transaction()?;
        let now = Self::now();
        let mut attachment_paths: Vec<String> = Vec::new();
        let mut tasks_deleted = 0usize;
        let mut tasks_moved = 0usize;

        if let Some(target) = &moved_to {
            for t in &tasks {
                self.conn.execute(
                    "UPDATE tasks SET project_id = ?1, updated_at = ?2 WHERE id = ?3",
                    params![target.id, now, t.id],
                )?;
                self.log(
                    "task",
                    t.id,
                    "UPDATE",
                    &format!(
                        "move {} to {} (project {} deleted)",
                        t.task_key, target.name, project.project_key
                    ),
                    &serde_json::json!({
                        "changes": format!("project: {} -> {}", project.name, target.name)
                    })
                    .to_string(),
                    Some(user),
                )?;
                tasks_moved += 1;
            }
        } else {
            for t in &tasks {
                {
                    let mut stmt = self
                        .conn
                        .prepare("SELECT stored_path FROM attachments WHERE task_id = ?1")?;
                    let rows = stmt.query_map([t.id], |r| r.get::<_, String>(0))?;
                    for r in rows {
                        attachment_paths.push(r?);
                    }
                }
                self.conn.execute("DELETE FROM attachments WHERE task_id = ?1", [t.id])?;
                self.conn.execute("DELETE FROM links WHERE task_id = ?1", [t.id])?;
                self.conn.execute("DELETE FROM prs WHERE task_id = ?1", [t.id])?;
                self.conn.execute("DELETE FROM note_diffs WHERE task_id = ?1", [t.id])?;
                self.conn.execute(
                    "DELETE FROM logs WHERE entity_type = 'task' AND entity_id = ?1",
                    [t.id],
                )?;
                self.conn.execute("DELETE FROM tasks WHERE id = ?1", [t.id])?;
                tasks_deleted += 1;
            }
        }

        {
            let mut stmt = self
                .conn
                .prepare("SELECT stored_path FROM attachments WHERE project_id = ?1")?;
            let rows = stmt.query_map([project_id], |r| r.get::<_, String>(0))?;
            for r in rows {
                attachment_paths.push(r?);
            }
        }
        self.conn.execute("DELETE FROM attachments WHERE project_id = ?1", [project_id])?;
        self.conn.execute("DELETE FROM links WHERE project_id = ?1", [project_id])?;
        self.conn.execute(
            "DELETE FROM logs WHERE entity_type = 'project' AND entity_id = ?1",
            [project_id],
        )?;
        self.conn.execute("DELETE FROM projects WHERE id = ?1", [project_id])?;

        // Leave a trace on the board: the deletion is logged against the
        // Unassigned project (P-000), which always exists.
        let unassigned = self.unassigned_project_id()?;
        let message = message.map(str::to_string).unwrap_or_else(|| {
            format!("delete project \"{}\" ({})", project.name, project.project_key)
        });
        self.log(
            "project",
            unassigned,
            "DELETE",
            &message,
            &serde_json::json!({
                "project_key": project.project_key,
                "name": project.name,
                "tasks_moved": tasks_moved,
                "tasks_deleted": tasks_deleted,
                "moved_to": moved_to.as_ref().map(|p| p.project_key.clone()),
            })
            .to_string(),
            Some(user),
        )?;
        tx.commit()?;
        Ok(ProjectDeleted {
            project,
            tasks_moved,
            tasks_deleted,
            moved_to,
            attachment_paths,
        })
    }

    /// All projects ordered by key.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_key, name, status, created_at, updated_at
                                          FROM projects ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], Self::project_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn project_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
        Ok(Project {
            id: r.get(0)?,
            project_key: r.get(1)?,
            name: r.get(2)?,
            status: r.get(3)?,
            created_at: r.get(4)?,
            updated_at: r.get(5)?,
        })
    }

    /// Lowercase Crockford base32 alphabet (digits + letters, minus the
    /// visually ambiguous i/l/o/u) used for short hash-style keys. Keeps
    /// keys short regardless of how many rows exist, unlike the old
    /// zero-padded sequential `T-00N` / `P-00N` / `PR-00N` scheme, which
    /// would eventually overflow its digit width.
    const KEY_ALPHABET: &'static [u8] = b"0123456789abcdefghjkmnpqrstvwxyz";

    /// A `len`-char pseudo-random suffix, salted by `attempt` so a caller
    /// can retry on a collision without generating the same value twice.
    fn random_key_suffix(len: usize, attempt: u64) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut hasher = Sha256::new();
        hasher.update(nanos.to_le_bytes());
        hasher.update(attempt.to_le_bytes());
        hasher.update(std::process::id().to_le_bytes());
        let digest = hasher.finalize();
        (0..len)
            .map(|i| Self::KEY_ALPHABET[digest[i] as usize % Self::KEY_ALPHABET.len()] as char)
            .collect()
    }

    /// Generate a `<prefix><4-char hash>` key that isn't already taken in
    /// `table`'s `key_column` (collision-checked, since the suffix is
    /// random rather than sequential).
    fn next_hash_key(&self, prefix: &str, table: &str, key_column: &str) -> Result<String> {
        for attempt in 0..1000u64 {
            let key = format!("{prefix}{}", Self::random_key_suffix(4, attempt));
            let exists: i64 = self.conn.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {key_column} = ?1"),
                [&key],
                |r| r.get(0),
            )?;
            if exists == 0 {
                return Ok(key);
            }
        }
        Err(DbError::new(
            "internal",
            format!("could not generate a unique {prefix}* key after 1000 attempts"),
        ))
    }

    fn next_project_key(&self) -> Result<String> {
        self.next_hash_key("P-", "projects", "project_key")
    }

    // ------------------------------------------------------------------
    // Tasks
    // ------------------------------------------------------------------

    /// Create a task. The key is the next free `T-00N`. When
    /// `spec.project` is absent the task falls back to P-000 "Unassigned"
    /// (an explicit row is written so FKs and queries stay uniform).
    /// Writes a `CREATE` log entry.
    pub fn create_task(&self, spec: TaskSpec) -> Result<Task> {
        let user = spec.user.as_deref().unwrap_or(DEFAULT_USER);
        let status = Self::normalize_task_status(spec.status.as_deref().unwrap_or(DEFAULT_TASK_STATUS));
        Self::validate_task_status(&status)?;
        let priority = spec.priority.unwrap_or_else(|| "low".to_string());
        let labels = spec.labels.unwrap_or_else(|| "[]".to_string());
        let key = self.next_task_key()?;
        let now = Self::now();

        let project_id = match spec.project {
            Some(pid) => Some(pid),
            None => Some(self.unassigned_project_id()?),
        };

        // New cards land at the bottom of their column.
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(COALESCE(position, id)), 0) + 1
             FROM tasks WHERE status = ?1 AND archived_at IS NULL",
            [status.clone()],
            |r| r.get(0),
        )?;

        self.conn.execute(
            "INSERT INTO tasks
                (task_key, project_id, title, status, priority, assignee, branch, labels, scope, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                key,
                project_id,
                spec.title,
                status,
                priority,
                spec.assignee,
                spec.branch,
                labels,
                spec.scope,
                position,
                now,
                now
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        let project_name: String = match project_id {
            Some(pid) => self
                .conn
                .query_row("SELECT name FROM projects WHERE id = ?1", [pid], |r| {
                    r.get(0)
                })
                .map_err(|e| DbError::new("db", format!("project {} vanished: {}", pid, e)))?,
            None => "Unassigned".to_string(),
        };
        let message = spec
            .message
            .unwrap_or_else(|| format!("add task \"{}\" to {}", spec.title, project_name));
        self.log(
            "task",
            id,
            "CREATE",
            &message,
            &format!(
                "{{\"project\": \"{}\", \"status\": \"{}\", \"priority\": \"{}\"}}",
                project_name, status, priority
            ),
            Some(user),
        )?;

        Ok(Task {
            id,
            task_key: key,
            project_id,
            title: spec.title,
            status,
            priority,
            assignee: spec.assignee,
            branch: spec.branch,
            labels,
            scope: spec.scope,
            notes: String::new(),
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        })
    }

    /// All tasks ordered by key.
    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        self.list_tasks_with(&TaskListSpec::default())
    }

    pub fn list_tasks_with(&self, spec: &TaskListSpec) -> Result<Vec<Task>> {
        let mut sql = String::from(
            "SELECT id, task_key, project_id, title, status, priority,
                    assignee, branch, labels, scope, notes, created_at, updated_at, archived_at
             FROM tasks",
        );
        let mut clauses = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        if !spec.include_archived {
            clauses.push("archived_at IS NULL".to_string());
        }
        if let Some(project_id) = spec.project_id {
            clauses.push("project_id = ?".to_string());
            values.push(Value::Integer(project_id));
        }
        if let Some(status) = &spec.status {
            clauses.push("status = ?".to_string());
            values.push(Value::Text(normalize_task_status(status)));
        }
        if let Some(priority) = &spec.priority {
            clauses.push("priority = ?".to_string());
            values.push(Value::Text(priority.clone()));
        }
        if let Some(assignee) = &spec.assignee {
            clauses.push("assignee = ?".to_string());
            values.push(Value::Text(assignee.clone()));
        }
        if let Some(scope) = &spec.scope {
            clauses.push("scope = ?".to_string());
            values.push(Value::Text(scope.clone()));
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY ");
        sql.push_str(match spec.sort {
            TaskSort::Key => "id ASC",
            TaskSort::Title => "title ASC, id ASC",
            TaskSort::Status => "CASE status WHEN 'planning' THEN 0 WHEN 'review' THEN 1 WHEN 'waiting-on' THEN 2 WHEN 'backlog' THEN 3 WHEN 'closed' THEN 4 END, id ASC",
            TaskSort::Priority => "priority ASC, id ASC",
            TaskSort::Updated => "updated_at DESC, id ASC",
            TaskSort::Board => "CASE status WHEN 'planning' THEN 0 WHEN 'review' THEN 1 WHEN 'waiting-on' THEN 2 WHEN 'backlog' THEN 3 WHEN 'closed' THEN 4 END, COALESCE(position, id) ASC, id ASC",
        });
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), Self::task_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn next_task_key(&self) -> Result<String> {
        self.next_hash_key("T-", "tasks", "task_key")
    }

    /// Look up a project by its `P-00N` key.
    pub fn project_by_key(&self, key: &str) -> Result<Project> {
        self.conn
            .query_row(
                "SELECT id, project_key, name, status, created_at, updated_at
                 FROM projects WHERE project_key = ?1",
                [key],
                Self::project_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("no project with key '{key}': {e}")))
    }

    /// Look up a project by exact name.
    pub fn project_by_name(&self, name: &str) -> Result<Project> {
        self.conn
            .query_row(
                "SELECT id, project_key, name, status, created_at, updated_at
                 FROM projects WHERE name = ?1",
                [name],
                Self::project_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("no project named '{name}': {e}")))
    }

    /// Look up a task by its `T-00N` key.
    pub fn task_by_key(&self, key: &str) -> Result<Task> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                    labels, scope, notes, created_at, updated_at, archived_at
             FROM tasks WHERE task_key = ?1",
        )?;
        let task = stmt
            .query_row([key], Self::task_from_row)
            .map_err(|e| DbError::new("not-found", format!("no task with key '{key}': {e}")))?;
        Ok(task)
    }

    /// Look up a task by its internal row id.
    pub fn task_by_id(&self, id: i64) -> Result<Task> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                    labels, scope, notes, created_at, updated_at, archived_at
             FROM tasks WHERE id = ?1",
        )?;
        let task = stmt
            .query_row([id], Self::task_from_row)
            .map_err(|e| DbError::new("not-found", format!("no task with id {id}: {e}")))?;
        Ok(task)
    }

    /// All tasks whose title exactly equals `title`. Zero, one, or many —
    /// the CLI layer treats >1 as ambiguous.
    pub fn tasks_by_title(&self, title: &str) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                        labels, scope, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE title = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([title], Self::task_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Tasks belonging to `project_id`, ordered by id.
    pub fn tasks_for_project(&self, project_id: i64) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                        labels, scope, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE project_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([project_id], Self::task_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Project name for a task (used in views and log lines).
    pub fn project_name_for_task(&self, task: &Task) -> String {
        match task.project_id {
            Some(pid) => self
                .conn
                .query_row("SELECT name FROM projects WHERE id = ?1", [pid], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap_or_else(|_| "Unassigned".to_string()),
            None => "Unassigned".to_string(),
        }
    }

    /// Replace a task's notes with new markdown text (Phase 7: the TUI's
    /// notes editor saves through here). Updates `updated_at` and writes an
    /// `UPDATE` log entry whose `details.changes` records the old/new state
    /// (`notes: empty -> 2 lines` / `notes: 2 lines -> 3 lines`), keeping
    /// the notes text itself out of `details` (the column IS the record).
    pub fn set_notes(
        &self,
        task_id: i64,
        notes: &str,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<()> {
        let user = user.unwrap_or(DEFAULT_USER);
        let now = Self::now();
        let task: Task = self
            .conn
            .query_row(
                "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                        labels, scope, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE id = ?1",
                [task_id],
                Self::task_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("task {task_id}: {e}")))?;

        let old_notes = task.notes.clone();

        // Notes, log entry and diff land together or not at all.
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute(
            "UPDATE tasks SET notes = ?1, updated_at = ?2 WHERE id = ?3",
            params![notes, now, task_id],
        )?;

        let describe = |t: &str| {
            let n = t.lines().count();
            if n == 0 {
                "empty".to_string()
            } else {
                format!("{n} line{}", if n == 1 { "" } else { "s" })
            }
        };
        let changes = format!("notes: {} -> {}", describe(&old_notes), describe(notes));
        let message = message
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("edit {}; update note", task.task_key));
        let log_id = self.log(
            "task",
            task_id,
            "UPDATE",
            &message,
            &format!("{{\"changes\": \"{}\"}}", Self::json_escape(&changes)),
            Some(user),
        )?;
        // Keep the previous text so this edit can be recovered by log hash.
        self.save_note_diff(task_id, &old_notes, notes, user, log_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Default commit message for an edit that names no message, derived from
    /// the fields the edit actually changes: `edit T-013; update note`,
    /// `edit T-013; update status to Review`, several joined with `; `.
    /// `lun task edit` uses this for its prompt default; `update_task` falls
    /// back to the same wording (web UI, TUI, API).
    pub fn default_edit_message(task: &Task, spec: &TaskUpdateSpec) -> String {
        let mut what: Vec<String> = Vec::new();
        if spec.title.as_ref().is_some_and(|v| *v != task.title) {
            what.push("update title".into());
        }
        if spec.project_id.is_some_and(|v| Some(v) != task.project_id) {
            what.push("update project".into());
        }
        if let Some(status) = &spec.status {
            let status = Self::normalize_task_status(status);
            if status != task.status {
                what.push(format!("update status to {}", Self::status_label(&status)));
            }
        }
        if let Some(p) = &spec.priority {
            if *p != task.priority {
                what.push(format!("update priority to {p}"));
            }
        }
        if let Some(a) = &spec.assignee {
            if *a != task.assignee {
                what.push(match a {
                    Some(a) => format!("update assignee to {a}"),
                    None => "clear assignee".into(),
                });
            }
        }
        if let Some(b) = &spec.branch {
            if *b != task.branch {
                what.push(match b {
                    Some(b) => format!("update branch to {b}"),
                    None => "clear branch".into(),
                });
            }
        }
        if spec.labels.as_ref().is_some_and(|v| *v != task.labels) {
            what.push("update labels".into());
        }
        if spec.scope.as_ref().is_some_and(|v| *v != task.scope) {
            what.push("update scope".into());
        }
        if spec.notes.as_ref().is_some_and(|v| *v != task.notes) {
            what.push("update note".into());
        }
        if what.is_empty() {
            format!("edit {}", task.task_key)
        } else {
            format!("edit {}; {}", task.task_key, what.join("; "))
        }
    }

    /// `status: planning -> review` becomes `update status to Review`; this is
    /// the default log message when the caller gives none.
    fn change_summary(change: &str) -> String {
        let (field, rest) = change.split_once(": ").unwrap_or((change, ""));
        let new = rest.rsplit_once(" -> ").map(|(_, n)| n).unwrap_or("").trim();
        match field {
            "notes" => "update note".to_string(),
            "status" => format!("update status to {}", Self::status_label(new)),
            "title" | "labels" | "scope" => format!("update {field}"),
            "assignee" | "branch" if new.is_empty() => format!("clear {field}"),
            _ => format!("update {field} to {new}"),
        }
    }

    /// `waiting-on` -> `Waiting-On` (how the board names its columns).
    fn status_label(status: &str) -> String {
        status
            .split('-')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn update_task(&self, task_id: i64, spec: TaskUpdateSpec) -> Result<Task> {
        let user = spec.user.as_deref().unwrap_or(DEFAULT_USER);
        let mut task = self.task_by_id(task_id)?;
        let now = Self::now();
        let mut changes = Vec::new();
        let mut note_diff_old: Option<String> = None;

        if let Some(title) = spec.title {
            if title != task.title {
                changes.push(format!("title: {} -> {}", task.title, title));
                task.title = title;
            }
        }
        if let Some(project_id) = spec.project_id {
            if Some(project_id) != task.project_id {
                changes.push(format!(
                    "project: {} -> {}",
                    self.project_name_for_task(&task),
                    self.conn
                        .query_row(
                            "SELECT name FROM projects WHERE id = ?1",
                            [project_id],
                            |r| { r.get::<_, String>(0) }
                        )
                        .map_err(|e| DbError::new(
                            "not-found",
                            format!("project {project_id}: {e}")
                        ))?
                ));
                task.project_id = Some(project_id);
            }
        }
        if let Some(status) = spec.status {
            let status = Self::normalize_task_status(&status);
            Self::validate_task_status(&status)?;
            if status != task.status {
                changes.push(format!("status: {} -> {}", task.status, status));
                task.status = status;
            }
        }
        if let Some(priority) = spec.priority {
            if priority != task.priority {
                changes.push(format!("priority: {} -> {}", task.priority, priority));
                task.priority = priority;
            }
        }
        if let Some(assignee) = spec.assignee {
            if assignee != task.assignee {
                changes.push(format!(
                    "assignee: {} -> {}",
                    task.assignee.as_deref().unwrap_or(""),
                    assignee.as_deref().unwrap_or("")
                ));
                task.assignee = assignee;
            }
        }
        if let Some(branch) = spec.branch {
            if branch != task.branch {
                changes.push(format!(
                    "branch: {} -> {}",
                    task.branch.as_deref().unwrap_or(""),
                    branch.as_deref().unwrap_or("")
                ));
                task.branch = branch;
            }
        }
        if let Some(labels) = spec.labels {
            if labels != task.labels {
                changes.push(format!("labels: {} -> {}", task.labels, labels));
                task.labels = labels;
            }
        }
        if let Some(scope) = spec.scope {
            if scope != task.scope {
                changes.push(format!(
                    "scope: {} -> {}",
                    task.scope.as_deref().unwrap_or(""),
                    scope.as_deref().unwrap_or("")
                ));
                task.scope = scope;
            }
        }
        if let Some(notes) = spec.notes {
            if notes != task.notes {
                let describe = |t: &str| {
                    let n = t.lines().count();
                    if n == 0 {
                        "empty".to_string()
                    } else {
                        format!("{n} line{}", if n == 1 { "" } else { "s" })
                    }
                };
                changes.push(format!(
                    "notes: {} -> {}",
                    describe(&task.notes),
                    describe(&notes)
                ));
                // Keep the old text; the diff is saved with the log entry below.
                note_diff_old = Some(std::mem::replace(&mut task.notes, notes.clone()));
            }
        }

        if changes.is_empty() {
            return Ok(task);
        }

        task.updated_at = now.clone();
        // Task row, log entry and note diff land together or not at all.
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute(
            "UPDATE tasks
                SET project_id = ?1, title = ?2, status = ?3, priority = ?4,
                    assignee = ?5, branch = ?6, labels = ?7, scope = ?8, notes = ?9, updated_at = ?10
              WHERE id = ?11",
            params![
                task.project_id,
                task.title,
                task.status,
                task.priority,
                task.assignee,
                task.branch,
                task.labels,
                task.scope,
                task.notes,
                now,
                task_id
            ],
        )?;
        // No caller message: say what changed, e.g. `edit T-013; update note`
        // or `edit T-013; update status to Review`.
        let message = spec.message.unwrap_or_else(|| {
            let what: Vec<String> = changes.iter().map(|c| Self::change_summary(c)).collect();
            format!("edit {}; {}", task.task_key, what.join("; "))
        });
        let log_id = self.log(
            "task",
            task_id,
            "UPDATE",
            &message,
            &format!(
                "{{\"changes\": \"{}\"}}",
                Self::json_escape(&changes.join(", "))
            ),
            Some(user),
        )?;
        if let Some(old) = note_diff_old {
            self.save_note_diff(task_id, &old, &task.notes, user, log_id)?;
        }
        tx.commit()?;
        self.task_by_id(task_id)
    }

    pub fn complete_task(
        &self,
        task_id: i64,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<Task> {
        let task = self.task_by_id(task_id)?;
        self.update_task(
            task_id,
            TaskUpdateSpec {
                status: Some(CLOSED_TASK_STATUS.to_string()),
                message: Some(
                    message
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("complete {}", task.task_key)),
                ),
                user: user.map(str::to_string),
                ..Default::default()
            },
        )
    }

    pub fn reopen_task(
        &self,
        task_id: i64,
        status: Option<&str>,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<Task> {
        let task = self.task_by_id(task_id)?;
        let target_status = Self::normalize_task_status(status.unwrap_or(DEFAULT_TASK_STATUS));
        Self::validate_task_status(&target_status)?;
        self.update_task(
            task_id,
            TaskUpdateSpec {
                status: Some(target_status.to_string()),
                message: Some(
                    message.map(str::to_string).unwrap_or_else(|| {
                        format!("reopen {} to {}", task.task_key, target_status)
                    }),
                ),
                user: user.map(str::to_string),
                ..Default::default()
            },
        )
    }

    pub fn archive_task(
        &self,
        task_id: i64,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<Task> {
        let user = user.unwrap_or(DEFAULT_USER);
        let task = self.task_by_id(task_id)?;
        if task.archived_at.is_some() {
            return Ok(task);
        }
        let now = Self::now();
        self.conn.execute(
            "UPDATE tasks SET archived_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )?;
        let message = message
            .map(str::to_string)
            .unwrap_or_else(|| format!("archive {}", task.task_key));
        self.log(
            "task",
            task_id,
            "ARCHIVE",
            &message,
            "{\"archived\": true}",
            Some(user),
        )?;
        self.task_by_id(task_id)
    }

    /// Restore a previously archived (deleted) task.
    pub fn unarchive_task(&self, task_id: i64, user: Option<&str>) -> Result<Task> {
        self.unarchive_task_with_message(task_id, None, user)
    }

    /// Like [`Lun::unarchive_task`] with a caller-supplied commit message.
    pub fn unarchive_task_with_message(
        &self,
        task_id: i64,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<Task> {
        let task = self.task_by_id(task_id)?;
        if task.archived_at.is_none() {
            return Ok(task);
        }
        let now = Self::now();
        self.conn.execute(
            "UPDATE tasks SET archived_at = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )?;
        self.log(
            "task",
            task_id,
            "UNARCHIVE",
            &message
                .map(str::to_string)
                .unwrap_or_else(|| format!("unarchive {}", task.task_key)),
            "{}",
            Some(user.unwrap_or(DEFAULT_USER)),
        )?;
        self.task_by_id(task_id)
    }

    /// Move a task one row down (dir = 1) or up (dir = -1) within its
    /// column by swapping `position` with its neighbor. No-op at the edge.
    pub fn reorder_task(&self, task_id: i64, dir: i64) -> Result<Task> {
        let task = self.task_by_id(task_id)?;
        let cmp = if dir > 0 { ">" } else { "<" };
        let order = if dir > 0 { "ASC" } else { "DESC" };
        let neighbor_id: Option<i64> = match self.conn.query_row(
            &format!(
                "SELECT id FROM tasks
                 WHERE status = ?1 AND archived_at IS NULL AND id != ?2
                   AND COALESCE(position, id) {cmp}
                       COALESCE((SELECT position FROM tasks WHERE id = ?2), ?3)
                 ORDER BY COALESCE(position, id) {order} LIMIT 1"
            ),
            params![task.status, task_id, task_id],
            |r| r.get(0),
        ) {
            Ok(id) => Some(id),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e.into()),
        };
        let Some(neighbor_id) = neighbor_id else {
            return Ok(task); // already at the edge of the column
        };
        let pos_a: Option<i64> =
            self.conn.query_row("SELECT position FROM tasks WHERE id = ?1", [task_id], |r| r.get(0))?;
        let pos_b: Option<i64> =
            self.conn.query_row("SELECT position FROM tasks WHERE id = ?1", [neighbor_id], |r| r.get(0))?;
        let now = Self::now();
        self.conn.execute(
            "UPDATE tasks SET position = ?1, updated_at = ?2 WHERE id = ?3",
            params![pos_b, now, task_id],
        )?;
        self.conn.execute(
            "UPDATE tasks SET position = ?1, updated_at = ?2 WHERE id = ?3",
            params![pos_a, now, neighbor_id],
        )?;
        let dir_name = if dir > 0 { "down" } else { "up" };
        self.log(
            "task",
            task_id,
            "REORDER",
            &format!("move {} {dir_name} in {}", task.task_key, task.status),
            "{}",
            Some(DEFAULT_USER),
        )?;
        self.task_by_id(task_id)
    }

    // ------------------------------------------------------------------
    // Log history
    // ------------------------------------------------------------------

    /// All log entries for one entity, newest first.
    pub fn logs_for(&self, entity_type: &str, entity_id: i64) -> Result<Vec<LogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, entity_type, entity_id, timestamp, user, action, message, details, hash
             FROM logs WHERE entity_type = ?1 AND entity_id = ?2 ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map(params![entity_type, entity_id], Self::log_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Log entries for every task in a project. Newest first.
    pub fn logs_for_project(&self, project_id: i64) -> Result<Vec<LogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.id, l.entity_type, l.entity_id, l.timestamp, l.user, l.action,
                    l.message, l.details, l.hash
             FROM logs l
             JOIN tasks t ON t.id = l.entity_id AND l.entity_type = 'task'
             WHERE t.project_id = ?1
             ORDER BY l.id DESC",
        )?;
        let rows = stmt
            .query_map([project_id], Self::log_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn log_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LogEntry> {
        Ok(LogEntry {
            id: r.get(0)?,
            entity_type: r.get(1)?,
            entity_id: r.get(2)?,
            timestamp: r.get(3)?,
            user: r.get(4)?,
            action: r.get(5)?,
            message: r.get(6)?,
            details: r.get(7)?,
            hash: r.get(8)?,
        })
    }

    /// All log entries across all entities, newest first.
    pub fn list_all_logs(&self) -> Result<Vec<LogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, entity_type, entity_id, timestamp, user, action, message, details, hash
             FROM logs ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([], Self::log_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Insert a log entry. Returns the new row id.
    pub fn insert_log(
        &self,
        entity_type: &str,
        entity_id: i64,
        user: &str,
        action: &str,
        message: &str,
        details: &str,
    ) -> Result<i64> {
        let timestamp = Self::now();
        let parent = self.parent_log_hash(entity_type, entity_id)?;
        let hash = Self::compute_log_hash(&parent, entity_type, entity_id, &timestamp, user, action, message, details);
        self.conn.execute(
            "INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![entity_type, entity_id, timestamp, user, action, message, details, hash],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Row id of the seeded P-000 "Unassigned" project.
    pub fn unassigned_project_id(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT id FROM projects WHERE project_key = 'P-000'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| {
                DbError::new(
                    "not-found",
                    format!("P-000 Unassigned project missing: {}", e),
                )
            })
    }

    fn task_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
        Ok(Task {
            id: r.get::<_, i64>(0)?,
            task_key: r.get::<_, String>(1)?,
            project_id: r.get::<_, Option<i64>>(2)?,
            title: r.get::<_, String>(3)?,
            status: r.get::<_, String>(4)?,
            priority: r.get::<_, String>(5)?,
            assignee: r.get::<_, Option<String>>(6)?,
            branch: r.get::<_, Option<String>>(7)?,
            labels: r.get::<_, String>(8)?,
            scope: r.get::<_, Option<String>>(9)?,
            notes: r.get::<_, String>(10)?,
            created_at: r.get::<_, String>(11)?,
            updated_at: r.get::<_, String>(12)?,
            archived_at: r.get::<_, Option<String>>(13)?,
        })
    }

    // ------------------------------------------------------------------
    // Attachments & links
    // ------------------------------------------------------------------

    /// Record an attachment for a task. The file itself is managed by
    /// callers (Phase 4 copies it into `.lun/attachments/`); this records
    /// the row and writes an `ATTACH` log entry.
    pub fn add_attachment(
        &self,
        task_id: i64,
        filename: &str,
        stored_path: &str,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<i64> {
        self.add_attachment_to(
            AttachmentTarget::Task(task_id),
            filename,
            stored_path,
            message,
            user,
        )
    }

    pub fn add_attachment_to(
        &self,
        target: AttachmentTarget,
        filename: &str,
        stored_path: &str,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<i64> {
        let user = user.unwrap_or(DEFAULT_USER);
        let now = Self::now();
        let (entity_type, entity_id, task_id, project_id, target_label) = match target {
            AttachmentTarget::Task(task_id) => {
                let task = self
                    .task_by_id(task_id)
                    .map_err(|e| DbError::new("not-found", format!("task {task_id}: {e}")))?;
                ("task", task_id, Some(task_id), None, task.task_key)
            }
            AttachmentTarget::Project(project_id) => {
                let project = self
                    .conn
                    .query_row(
                        "SELECT id, project_key, name, status, created_at, updated_at
                     FROM projects WHERE id = ?1",
                        [project_id],
                        Self::project_from_row,
                    )
                    .map_err(|e| DbError::new("not-found", format!("project {project_id}: {e}")))?;
                (
                    "project",
                    project_id,
                    None,
                    Some(project_id),
                    project.project_key,
                )
            }
        };
        self.conn.execute(
            "INSERT INTO attachments (task_id, project_id, filename, stored_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_id, project_id, filename, stored_path, now],
        )?;
        let id = self.conn.last_insert_rowid();

        let message = message
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("attach \"{}\" to {}", filename, target_label));
        self.log(
            entity_type,
            entity_id,
            "ATTACH",
            &message,
            &format!(
                "{{\"filename\": \"{}\", \"stored_path\": \"{}\"}}",
                Self::json_escape(filename),
                Self::json_escape(stored_path)
            ),
            Some(user),
        )?;
        Ok(id)
    }

    /// Add a link to a task or a project. Writes a `LINK` log entry on the
    /// target entity.
    pub fn add_link(
        &self,
        target: LinkTarget,
        label: &str,
        uri: &str,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<i64> {
        let user = user.unwrap_or(DEFAULT_USER);
        let now = Self::now();
        let (entity_type, entity_id, task_id, project_id) = match target {
            LinkTarget::Task(tid) => ("task", tid, Some(tid), None),
            LinkTarget::Project(pid) => ("project", pid, None, Some(pid)),
        };

        self.conn.execute(
            "INSERT INTO links (task_id, project_id, label, uri, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_id, project_id, label, uri, now],
        )?;
        let id = self.conn.last_insert_rowid();

        let message = message
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("link \"{}\" ({uri}) to {entity_type} #{entity_id}", label));
        self.log(
            entity_type,
            entity_id,
            "LINK",
            &message,
            &serde_json::json!({"label": label, "uri": uri}).to_string(),
            Some(user),
        )?;
        Ok(id)
    }

    /// Delete a link. Writes an `UNLINK` log entry on the owning task/project
    /// that carries the label and uri, so the link can be restored later.
    pub fn delete_link(&self, link_id: i64, user: Option<&str>) -> Result<Link> {
        let user = user.unwrap_or(DEFAULT_USER);
        let link = self
            .conn
            .query_row(
                "SELECT id, task_id, project_id, label, uri, created_at FROM links WHERE id = ?1",
                [link_id],
                Self::link_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("link {link_id}: {e}")))?;
        let (entity_type, entity_id) = match (link.task_id, link.project_id) {
            (Some(t), _) => ("task", t),
            (_, Some(p)) => ("project", p),
            _ => return Err(DbError::new("db", "link has no owner")),
        };
        self.conn.execute("DELETE FROM links WHERE id = ?1", [link_id])?;
        let details = serde_json::json!({"label": link.label, "uri": link.uri}).to_string();
        self.log(
            entity_type,
            entity_id,
            "UNLINK",
            &format!("unlinked \"{}\" ({}) from {entity_type} #{entity_id}", link.label, link.uri),
            &details,
            Some(user),
        )?;
        Ok(link)
    }

    /// Restore the most recently deleted link of a task/project that has not
    /// been re-added since (an `UNLINK` log entry with no later `LINK` for the
    /// same uri). Returns `None` when there is nothing to restore.
    pub fn restore_last_link(
        &self,
        target: LinkTarget,
        user: Option<&str>,
    ) -> Result<Option<Link>> {
        let (entity_type, entity_id) = match target {
            LinkTarget::Task(t) => ("task", t),
            LinkTarget::Project(p) => ("project", p),
        };
        // Newest first.
        let logs = self.logs_for(entity_type, entity_id)?;
        let parse = |d: &str| -> Option<(String, String)> {
            let v: serde_json::Value = serde_json::from_str(d).ok()?;
            Some((v.get("label")?.as_str()?.to_string(), v.get("uri")?.as_str()?.to_string()))
        };
        // uri -> number of LINK entries seen newer than the log row under
        // inspection; each one cancels out one older UNLINK for that uri.
        let mut readded: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for entry in &logs {
            let Some((label, uri)) = parse(&entry.details) else { continue };
            match entry.action.as_str() {
                "LINK" => *readded.entry(uri).or_insert(0) += 1,
                "UNLINK" => {
                    let n = readded.entry(uri.clone()).or_insert(0);
                    if *n > 0 {
                        *n -= 1;
                        continue;
                    }
                    let message = format!("restored link \"{label}\" ({uri}) to {entity_type} #{entity_id}");
                    let id = self.add_link(target, &label, &uri, Some(&message), user)?;
                    let link = self
                        .conn
                        .query_row(
                            "SELECT id, task_id, project_id, label, uri, created_at FROM links WHERE id = ?1",
                            [id],
                            Self::link_from_row,
                        )
                        .map_err(|e| DbError::new("db", format!("reading restored link: {e}")))?;
                    return Ok(Some(link));
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// List attachments recorded for a task, oldest first.
    pub fn attachments_for_task(&self, task_id: i64) -> Result<Vec<Attachment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, project_id, filename, stored_path, created_at
             FROM attachments WHERE task_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([task_id], |r| {
                Ok(Attachment {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    project_id: r.get(2)?,
                    filename: r.get(3)?,
                    stored_path: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })
            .map_err(|e| DbError::new("db", format!("listing attachments: {e}")))?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(|e| DbError::new("db", format!("listing attachments: {e}")))
    }

    pub fn attachments_for_project(&self, project_id: i64) -> Result<Vec<Attachment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, project_id, filename, stored_path, created_at
             FROM attachments WHERE project_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([project_id], |r| {
                Ok(Attachment {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    project_id: r.get(2)?,
                    filename: r.get(3)?,
                    stored_path: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })
            .map_err(|e| DbError::new("db", format!("listing attachments: {e}")))?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(|e| DbError::new("db", format!("listing attachments: {e}")))
    }

    pub fn remove_attachment(
        &self,
        attachment_id: i64,
        message: Option<&str>,
        user: Option<&str>,
    ) -> Result<Attachment> {
        let user = user.unwrap_or(DEFAULT_USER);
        let attachment = self
            .conn
            .query_row(
                "SELECT id, task_id, project_id, filename, stored_path, created_at
             FROM attachments WHERE id = ?1",
                [attachment_id],
                |r| {
                    Ok(Attachment {
                        id: r.get(0)?,
                        task_id: r.get(1)?,
                        project_id: r.get(2)?,
                        filename: r.get(3)?,
                        stored_path: r.get(4)?,
                        created_at: r.get(5)?,
                    })
                },
            )
            .map_err(|e| DbError::new("not-found", format!("attachment {attachment_id}: {e}")))?;
        self.conn
            .execute("DELETE FROM attachments WHERE id = ?1", [attachment_id])?;
        let (entity_type, entity_id, target_label) =
            match (attachment.task_id, attachment.project_id) {
                (Some(task_id), None) => {
                    let task = self.task_by_id(task_id)?;
                    ("task", task_id, task.task_key)
                }
                (None, Some(project_id)) => {
                    let project = self.conn.query_row(
                        "SELECT id, project_key, name, status, created_at, updated_at
                    FROM projects WHERE id = ?1",
                        [project_id],
                        Self::project_from_row,
                    )?;
                    ("project", project_id, project.project_key)
                }
                _ => {
                    return Err(DbError::new(
                        "db",
                        format!("attachment {} has invalid ownership", attachment_id),
                    ))
                }
            };
        let message = message.map(str::to_string).unwrap_or_else(|| {
            format!(
                "remove attachment \"{}\" from {}",
                attachment.filename, target_label
            )
        });
        self.log(
            entity_type,
            entity_id,
            "DETACH",
            &message,
            &format!(
                "{{\"filename\": \"{}\", \"stored_path\": \"{}\"}}",
                Self::json_escape(&attachment.filename),
                Self::json_escape(&attachment.stored_path)
            ),
            Some(user),
        )?;
        Ok(attachment)
    }

    /// List links on a task, oldest first.
    pub fn links_for_task(&self, task_id: i64) -> Result<Vec<Link>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, project_id, label, uri, created_at
             FROM links WHERE task_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([task_id], Self::link_from_row)
            .map_err(|e| DbError::new("db", format!("listing links: {e}")))?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(|e| DbError::new("db", format!("listing links: {e}")))
    }

    /// List links on a project, oldest first.
    pub fn links_for_project(&self, project_id: i64) -> Result<Vec<Link>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, project_id, label, uri, created_at
             FROM links WHERE project_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([project_id], Self::link_from_row)
            .map_err(|e| DbError::new("db", format!("listing links: {e}")))?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(|e| DbError::new("db", format!("listing links: {e}")))
    }

    fn link_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Link> {
        Ok(Link {
            id: r.get(0)?,
            task_id: r.get(1)?,
            project_id: r.get(2)?,
            label: r.get(3)?,
            uri: r.get(4)?,
            created_at: r.get(5)?,
        })
    }

    // ------------------------------------------------------------------
    // PRs (Phase 9)
    // ------------------------------------------------------------------

    fn pr_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Pr> {
        Ok(Pr {
            id: r.get(0)?,
            pr_key: r.get(1)?,
            task_id: r.get(2)?,
            source_branch: r.get(3)?,
            target_branch: r.get(4)?,
            status: r.get(5)?,
            created_at: r.get(6)?,
            merged_at: r.get(7)?,
        })
    }

    /// All PRs ordered by key (PR-001 first).
    pub fn list_prs(&self) -> Result<Vec<Pr>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pr_key, task_id, source_branch, target_branch, status, created_at, merged_at
             FROM prs ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], Self::pr_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Open PRs only (`status = 'open'`), ordered by key.
    pub fn list_open_prs(&self) -> Result<Vec<Pr>> {
        Ok(self
            .list_prs()?
            .into_iter()
            .filter(|p| p.status == "open")
            .collect())
    }

    /// Look up a PR by its `PR-00N` key.
    pub fn pr_by_key(&self, key: &str) -> Result<Pr> {
        self.conn
            .query_row(
                "SELECT id, pr_key, task_id, source_branch, target_branch, status, created_at, merged_at
                 FROM prs WHERE pr_key = ?1",
                [key],
                Self::pr_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("no PR with key '{key}': {e}")))
    }

    /// PRs for one task, oldest first.
    pub fn prs_for_task(&self, task_id: i64) -> Result<Vec<Pr>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pr_key, task_id, source_branch, target_branch, status, created_at, merged_at
             FROM prs WHERE task_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([task_id], Self::pr_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The latest open PR for a task, if any (used by `lun task`'s
    /// "PRs" section and as the default target of `lun pr merge`).
    pub fn open_pr_for_task(&self, task_id: i64) -> Result<Option<Pr>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pr_key, task_id, source_branch, target_branch, status, created_at, merged_at
             FROM prs WHERE task_id = ?1 AND status = 'open' ORDER BY id DESC",
        )?;
        let mut rows = stmt.query_map([task_id], Self::pr_from_row)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Open a PR for a task. `source_branch` defaults to the task's
    /// `branch` field (the task's branch IS the PR's branch — lun's
    /// kanban tracks the branch per task); `target_branch` defaults to
    /// `main`. Writes an `UPDATE` log entry on the task carrying the PR's
    /// key in `details` (`pr`/`source`/`target`) — lun's `logs` table is
    /// CHECK-constrained to project/task entities, so the PR lifecycle
    /// lives in the task's log (the "map logs entries to the PR lifecycle"
    /// step of the plan).
    pub fn create_pr(&self, spec: PrSpec) -> Result<Pr> {
        let user = spec.user.as_deref().unwrap_or(DEFAULT_USER);
        let task: Task = self
            .conn
            .query_row(
                "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                        labels, scope, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE id = ?1",
                [spec.task_id],
                Self::task_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("task {}: {e}", spec.task_id)))?;

        // One open PR per task: re-opening is an error, not a duplicate.
        if self.open_pr_for_task(task.id)?.is_some() {
            return Err(DbError::new(
                "usage",
                format!(
                    "{} already has an open PR — merge it first (`lun pr merge`)",
                    task.task_key
                ),
            ));
        }
        let source = match spec.source_branch {
            Some(b) if !b.trim().is_empty() => b,
            _ => task
                .branch
                .clone()
                .filter(|b| !b.trim().is_empty())
                .ok_or_else(|| {
                    DbError::new(
                        "usage",
                        format!(
                            "{} has no branch — pass --from <branch> or set the task's branch first",
                            task.task_key
                        ),
                    )
                })?,
        };
        let target = spec.target_branch.unwrap_or_else(|| "main".to_string());
        if source == target {
            return Err(DbError::new(
                "usage",
                format!("source and target branch are both '{source}'"),
            ));
        }

        let key = self.next_hash_key("PR-", "prs", "pr_key")?;
        let now = Self::now();

        self.conn.execute(
            "INSERT INTO prs (pr_key, task_id, source_branch, target_branch, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'open', ?5)",
            params![key, task.id, source, target, now],
        )?;
        let id = self.conn.last_insert_rowid();

        let message = spec.message.unwrap_or_else(|| {
            format!(
                "open {key} from {source} into {target} for {}",
                task.task_key
            )
        });
        // PR's own log entity is a task-type entry on the PR row? No —
        // logs.entity_type is CHECK-constrained to project|task, so the
        // PR lifecycle is recorded on the TASK (that IS the "map logs
        // entries to the PR lifecycle" the plan asks for).
        self.log(
            "task",
            task.id,
            "UPDATE",
            &message,
            &format!("{{\"pr\": \"{key}\", \"source\": \"{source}\", \"target\": \"{target}\"}}"),
            Some(user),
        )?;

        Ok(Pr {
            id,
            pr_key: key,
            task_id: task.id,
            source_branch: source,
            target_branch: target,
            status: "open".to_string(),
            created_at: now,
            merged_at: None,
        })
    }

    /// Merge an open PR: flip its status to `merged`, stamp `merged_at`,
    /// and move the task to `done` (a merged PR means the task is done —
    /// the git-style lifecycle). Logs `MERGE` on the task with
    /// `details` carrying the PR key + branches.
    ///
    /// `message` defaults to `merge <pr> (<source> -> <target>)`.
    pub fn merge_pr(&self, pr_id: i64, message: Option<&str>, user: Option<&str>) -> Result<Pr> {
        let user = user.unwrap_or(DEFAULT_USER);
        let now = Self::now();
        let pr: Pr = self
            .conn
            .query_row(
                "SELECT id, pr_key, task_id, source_branch, target_branch, status, created_at, merged_at
                 FROM prs WHERE id = ?1",
                [pr_id],
                Self::pr_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("no PR with id {pr_id}: {e}")))?;
        if pr.status != "open" {
            return Err(DbError::new(
                "usage",
                format!("{} is already {status}", pr.pr_key, status = pr.status),
            ));
        }
        self.conn.execute(
            "UPDATE prs SET status = 'merged', merged_at = ?1 WHERE id = ?2",
            params![now, pr_id],
        )?;

        let task: Task = self
            .conn
            .query_row(
                "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                        labels, scope, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE id = ?1",
                [pr.task_id],
                Self::task_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("task {}: {e}", pr.task_id)))?;

        // Merged PR -> task closed (the logical-merge step; `git merge`
        // itself is optional glue the CLI may run on top of this).
        let changes = if task.status == CLOSED_TASK_STATUS {
            format!("PR {}: merged", pr.pr_key)
        } else {
            format!("Status: {} -> closed, PR {}: merged", task.status, pr.pr_key)
        };
        self.conn.execute(
            "UPDATE tasks SET status = 'closed', updated_at = ?1 WHERE id = ?2",
            params![now, task.id],
        )?;

        let message = message.map(|m| m.to_string()).unwrap_or_else(|| {
            format!(
                "merge {} ({} -> {})",
                pr.pr_key, pr.source_branch, pr.target_branch
            )
        });
        self.log(
            "task",
            task.id,
            "MERGE",
            &message,
            &format!(
                "{{\"changes\": \"{changes}\", \"pr\": \"{}\", \"source\": \"{}\", \"target\": \"{}\"}}",
                pr.pr_key, pr.source_branch, pr.target_branch
            ),
            Some(user),
        )?;

        Ok(Pr {
            status: "merged".to_string(),
            merged_at: Some(now),
            ..pr
        })
    }

    // ------------------------------------------------------------------
    // Web API helpers
    // ------------------------------------------------------------------

    /// List all projects (including inactive).
    pub fn list_all_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_key, name, status, created_at, updated_at
             FROM projects ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], Self::project_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// List all unique scopes used across non-archived tasks.
    pub fn list_all_scopes(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT scope FROM tasks
             WHERE scope IS NOT NULL AND scope != '' AND archived_at IS NULL
             ORDER BY scope",
        )?;
        let rows = stmt
            .query_map([], |r| Ok(r.get::<_, String>(0)?))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Search tasks by free-text across title, notes, and labels.
    pub fn search_tasks(&self, query: &str) -> Result<Vec<Task>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT id, task_key, project_id, title, status, priority,
                    assignee, branch, labels, scope, notes, created_at, updated_at, archived_at
             FROM tasks
             WHERE archived_at IS NULL
               AND (title LIKE ?1 OR notes LIKE ?1 OR labels LIKE ?1)
             ORDER BY task_key ASC",
        )?;
        let rows = stmt
            .query_map([&pattern], Self::task_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Convenience wrappers used by the web API
    // -----------------------------------------------------------------------

    /// Logs for a single task.
    pub fn task_logs(&self, task_id: i64) -> Result<Vec<LogEntry>> {
        self.logs_for("task", task_id)
    }

    /// Logs for a single project (including task logs within it).
    pub fn project_logs(&self, project_id: i64) -> Result<Vec<LogEntry>> {
        let mut all = self.logs_for("project", project_id)?;
        all.extend(self.logs_for_project(project_id)?);
        all.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(all)
    }

    /// Attachments for a single task.
    pub fn task_attachments(&self, task_id: i64) -> Result<Vec<Attachment>> {
        self.attachments_for_task(task_id)
    }

    /// Attachment by its row id.
    pub fn attachment_by_id(&self, id: i64) -> Result<Attachment> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, project_id, filename, stored_path, created_at FROM attachments WHERE id = ?1",
        )?;
        let row = stmt.query_row([id], |r| {
            Ok(Attachment {
                id: r.get(0)?,
                task_id: r.get(1)?,
                project_id: r.get(2)?,
                filename: r.get(3)?,
                stored_path: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(row)
    }

    /// Links for a single task.
    pub fn task_links(&self, task_id: i64) -> Result<Vec<Link>> {
        self.links_for_task(task_id)
    }

    /// PRs for a single task.
    pub fn task_prs(&self, task_id: i64) -> Result<Vec<Pr>> {
        self.prs_for_task(task_id)
    }
}

/// Format a unix-seconds timestamp as `YYYY-MM-DDTHH:MM:SSZ` (UTC).
/// Hand-rolled to keep Phase 2 dependency-free apart from rusqlite.
fn format_epoch(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days algorithm (Howard Hinnant).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let mo = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}
