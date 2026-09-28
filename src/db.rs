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

use rusqlite::{params, types::Value, Connection};
use std::path::Path;

pub const CURRENT_VERSION: i64 = 6;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: i64,
    pub task_id: Option<i64>,
    pub project_id: Option<i64>,
    pub filename: String,
    pub stored_path: String,
    pub created_at: String,
}

/// Row of the `links` table (read model).
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// `todo`, `doing`, `follow-up`, `blocked`, or `done`. Defaults to `todo`.
    pub status: Option<String>,
    /// `low`, `med`, `high`. Defaults to `low`.
    pub priority: Option<String>,
    pub assignee: Option<String>,
    pub branch: Option<String>,
    /// JSON array of label strings, e.g. `["cli", "db"]`. Defaults to `[]`.
    pub labels: Option<String>,
    /// Commit-style message for the log entry. Defaults to `add task "<title>" to <project>`.
    pub message: Option<String>,
    pub user: Option<String>,
}

/// Row of the `prs` table (read model; Phase 9).
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: i64,
    pub project_key: String,
    pub name: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: i64,
    pub task_key: String,
    pub project_id: Option<i64>,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub assignee: Option<String>,
    pub branch: Option<String>,
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
}

#[derive(Debug, Clone, Default)]
pub struct TaskListSpec {
    pub project_id: Option<i64>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<String>,
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
        match status {
            "in-progress" => "doing".to_string(),
            "review" => "follow-up".to_string(),
            other => other.to_string(),
        }
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
        if matches!(status, "todo" | "doing" | "follow-up" | "blocked" | "done") {
            Ok(())
        } else {
            Err(DbError::new(
                "invalid",
                format!(
                    "invalid status '{status}' (expected todo, doing, follow-up, blocked, or done)"
                ),
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
                status      TEXT NOT NULL DEFAULT 'todo'
                           CHECK (status IN ('todo', 'doing', 'follow-up', 'blocked', 'done')),
                priority    TEXT NOT NULL DEFAULT 'low'
                           CHECK (priority IN ('low', 'med', 'high')),
                assignee    TEXT,
                branch      TEXT,
                labels      TEXT NOT NULL DEFAULT '[]',
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                notes       TEXT NOT NULL DEFAULT '',
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
             INSERT INTO projects (project_key, name, status, created_at, updated_at)
             VALUES ('P-000', 'Unassigned', 'active',
                     (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));
             INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details)
             VALUES ('project', 1,
                     (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     'system', 'CREATE',
                     'seed P-000 \"Unassigned\" project',
                     '{\"seed\": true}');
             INSERT INTO migrations (version, applied_at)
                 VALUES (6, (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')));",
        )?;
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
                   created_at  TEXT NOT NULL,
                   updated_at  TEXT NOT NULL,
                   notes       TEXT NOT NULL DEFAULT '',
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
                INSERT INTO tasks (id, task_key, project_id, title, status, priority, assignee, branch, labels, created_at, updated_at, notes, archived_at)
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
                       created_at,
                       updated_at,
                       notes,
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

    fn now() -> String {
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
        self.conn.execute(
            "INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entity_type,
                entity_id,
                Self::now(),
                user.unwrap_or(DEFAULT_USER),
                action,
                message,
                details,
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

    /// All projects ordered by key.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_key, name, status, created_at, updated_at
                                          FROM projects ORDER BY project_key",
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

    fn next_project_key(&self) -> Result<String> {
        let n: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(id), 0) FROM projects", [], |r| {
                r.get(0)
            })?;
        // The P-000 seed is row id 1, so the key number is the new row's id
        // minus one: first real project (id 2) is P-001, not P-002.
        Ok(format!("P-{:03}", n))
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
        let status = Self::normalize_task_status(spec.status.as_deref().unwrap_or("todo"));
        Self::validate_task_status(&status)?;
        let priority = spec.priority.unwrap_or_else(|| "low".to_string());
        let labels = spec.labels.unwrap_or_else(|| "[]".to_string());
        let key = self.next_task_key()?;
        let now = Self::now();

        let project_id = match spec.project {
            Some(pid) => Some(pid),
            None => Some(self.unassigned_project_id()?),
        };

        self.conn.execute(
            "INSERT INTO tasks
                (task_key, project_id, title, status, priority, assignee, branch, labels, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                key,
                project_id,
                spec.title,
                status,
                priority,
                spec.assignee,
                spec.branch,
                labels,
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
                    assignee, branch, labels, notes, created_at, updated_at, archived_at
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
            values.push(Value::Text(status.clone()));
        }
        if let Some(priority) = &spec.priority {
            clauses.push("priority = ?".to_string());
            values.push(Value::Text(priority.clone()));
        }
        if let Some(assignee) = &spec.assignee {
            clauses.push("assignee = ?".to_string());
            values.push(Value::Text(assignee.clone()));
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY ");
        sql.push_str(match spec.sort {
            TaskSort::Key => "task_key ASC",
            TaskSort::Title => "title ASC, task_key ASC",
            TaskSort::Status => "status ASC, task_key ASC",
            TaskSort::Priority => "priority ASC, task_key ASC",
            TaskSort::Updated => "updated_at DESC, task_key ASC",
        });
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), Self::task_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn next_task_key(&self) -> Result<String> {
        let n: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(id), 0) FROM tasks", [], |r| r.get(0))?;
        Ok(format!("T-{:03}", n + 1))
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
        self.conn
            .query_row(
                "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                        labels, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE task_key = ?1",
                [key],
                Self::task_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("no task with key '{key}': {e}")))
    }

    /// Look up a task by its internal row id.
    pub fn task_by_id(&self, id: i64) -> Result<Task> {
        self.conn
            .query_row(
                "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                        labels, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE id = ?1",
                [id],
                Self::task_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("no task with id {id}: {e}")))
    }

    /// All tasks whose title exactly equals `title`. Zero, one, or many —
    /// the CLI layer treats >1 as ambiguous.
    pub fn tasks_by_title(&self, title: &str) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_key, project_id, title, status, priority, assignee, branch,
                        labels, notes, created_at, updated_at, archived_at
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
                        labels, notes, created_at, updated_at, archived_at
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
                        labels, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE id = ?1",
                [task_id],
                Self::task_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("task {task_id}: {e}")))?;

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
        let changes = format!("notes: {} -> {}", describe(&task.notes), describe(notes));
        let message = message
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("update notes for {}", task.task_key));
        self.log(
            "task",
            task_id,
            "UPDATE",
            &message,
            &format!("{{\"changes\": \"{}\"}}", Self::json_escape(&changes)),
            Some(user),
        )?;
        Ok(())
    }

    pub fn update_task(&self, task_id: i64, spec: TaskUpdateSpec) -> Result<Task> {
        let user = spec.user.as_deref().unwrap_or(DEFAULT_USER);
        let mut task = self.task_by_id(task_id)?;
        let now = Self::now();
        let mut changes = Vec::new();

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
                task.notes = notes;
            }
        }

        if changes.is_empty() {
            return Ok(task);
        }

        task.updated_at = now.clone();
        self.conn.execute(
            "UPDATE tasks
                SET project_id = ?1, title = ?2, status = ?3, priority = ?4,
                    assignee = ?5, branch = ?6, labels = ?7, notes = ?8, updated_at = ?9
              WHERE id = ?10",
            params![
                task.project_id,
                task.title,
                task.status,
                task.priority,
                task.assignee,
                task.branch,
                task.labels,
                task.notes,
                now,
                task_id
            ],
        )?;
        let message = spec
            .message
            .unwrap_or_else(|| format!("edit {}", task.task_key));
        self.log(
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
                status: Some("done".to_string()),
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
        let target_status = Self::normalize_task_status(status.unwrap_or("doing"));
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

    // ------------------------------------------------------------------
    // Log history
    // ------------------------------------------------------------------

    /// All log entries for one entity, newest first.
    pub fn logs_for(&self, entity_type: &str, entity_id: i64) -> Result<Vec<LogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, entity_type, entity_id, timestamp, user, action, message, details
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
                    l.message, l.details
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
        })
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
            id: r.get(0)?,
            task_key: r.get(1)?,
            project_id: r.get(2)?,
            title: r.get(3)?,
            status: r.get(4)?,
            priority: r.get(5)?,
            assignee: r.get(6)?,
            branch: r.get(7)?,
            labels: r.get(8)?,
            notes: r.get(9)?,
            created_at: r.get(10)?,
            updated_at: r.get(11)?,
            archived_at: r.get(12)?,
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
            &format!("{{\"label\": \"{}\", \"uri\": \"{}\"}}", label, uri),
            Some(user),
        )?;
        Ok(id)
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
                        labels, notes, created_at, updated_at, archived_at
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

        let key = format!(
            "PR-{:03}",
            self.conn
                .query_row("SELECT COALESCE(MAX(id), 0) FROM prs", [], |r| r
                    .get::<_, i64>(0),)?
                + 1
        );
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
                        labels, notes, created_at, updated_at, archived_at
                 FROM tasks WHERE id = ?1",
                [pr.task_id],
                Self::task_from_row,
            )
            .map_err(|e| DbError::new("not-found", format!("task {}: {e}", pr.task_id)))?;

        // Merged PR -> task done (the logical-merge step; `git merge`
        // itself is optional glue the CLI may run on top of this).
        let changes = if task.status == "done" {
            format!("PR {}: merged", pr.pr_key)
        } else {
            format!("Status: {} -> done, PR {}: merged", task.status, pr.pr_key)
        };
        self.conn.execute(
            "UPDATE tasks SET status = 'done', updated_at = ?1 WHERE id = ?2",
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
