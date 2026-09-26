//! Phase 2: SQLite schema, migrations, and the data-access layer.
//!
//! Canonical storage is the DB (`.lun/lun.db`); markdown is a view, never a
//! source of truth. Every state-changing operation writes a `logs` entry
//! with a commit-style message (the "log-on-write" guarantee).

use rusqlite::{params, Connection};
use std::path::Path;

pub const CURRENT_VERSION: i64 = 1;

/// Default actor for log entries (Phase 2 has no user-profile table yet).
const DEFAULT_USER: &str = "me";

#[derive(Debug)]
pub struct DbError {
    kind: &'static str,
    message: String,
}

impl DbError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
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

/// Input for [`Lun::create_project`].
#[derive(Debug, Clone, Default)]
pub struct ProjectSpec {
    pub name: String,
    /// `planning`, `active`, `in-progress`, or `done`. Defaults to `active`.
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
    /// `todo`, `in-progress`, `review`, `done`. Defaults to `todo`.
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
    pub created_at: String,
    pub updated_at: String,
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
                format!(
                    "{} does not exist; run `lun init` first",
                    path.display()
                ),
            ));
        }
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self { conn })
    }

    fn migrate(conn: &mut Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS migrations (
                version    INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
             );",
        )?;

        let version: i64 = conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM migrations", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);

        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE projects (
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

        let version: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM migrations",
            [],
            |r| r.get(0),
        )?;
        if version != CURRENT_VERSION {
            return Err(DbError::new(
                "version-mismatch",
                format!(
                    "database is at schema version {version}, lun expects {CURRENT_VERSION}"
                ),
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
        let status = spec.status.unwrap_or_else(|| "active".to_string());
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

    /// All projects ordered by key.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare("SELECT id, project_key, name, status, created_at, updated_at
                                          FROM projects ORDER BY project_key")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Project {
                    id: r.get(0)?,
                    project_key: r.get(1)?,
                    name: r.get(2)?,
                    status: r.get(3)?,
                    created_at: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
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
        let status = spec.status.unwrap_or_else(|| "todo".to_string());
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
        let message = spec.message.unwrap_or_else(|| {
            format!("add task \"{}\" to {}", spec.title, project_name)
        });
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
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// All tasks ordered by key.
    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_key, project_id, title, status, priority,
                    assignee, branch, labels, created_at, updated_at
             FROM tasks ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], Self::task_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn next_task_key(&self) -> Result<String> {
        let n: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(id), 0) FROM tasks",
            [],
            |r| r.get(0),
        )?;
        Ok(format!("T-{:03}", n + 1))
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
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
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
        let user = user.unwrap_or(DEFAULT_USER);
        let now = Self::now();
        let task: Task = self
            .conn
            .query_row("SELECT * FROM tasks WHERE id = ?1", [task_id], Self::task_from_row)
            .map_err(|e| DbError::new("not-found", format!("task {}: {}", task_id, e)))?;

        self.conn.execute(
            "INSERT INTO attachments (task_id, filename, stored_path, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![task_id, filename, stored_path, now],
        )?;
        let id = self.conn.last_insert_rowid();

        let message = message
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("attach \"{}\" to {}", filename, task.task_key));
        self.log(
            "task",
            task_id,
            "ATTACH",
            &message,
            &format!("{{\"filename\": \"{}\", \"stored_path\": \"{}\"}}", filename, stored_path),
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
