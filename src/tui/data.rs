//! Snapshot of everything the TUI renders, loaded from the DB once per
//! view switch. Phase 5 kept the TUI read-only on projects/tasks; Phase 6
//! also snapshots task logs, project logs, attachments, and links so the
//! task view and log view render entirely from this snapshot (no DB access
//! in the render path).

use crate::db::{Attachment, DbError, Link, LogEntry, Lun, Project, Task};

/// Everything one TUI frame needs.
#[derive(Debug, Clone, Default)]
pub struct TuiData {
    pub version: String,
    pub repo_path: String,
    /// Git branch, or "<none>" when not inside a git repo.
    pub branch: String,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    /// Index into [`TuiData::projects`] for the current project.
    pub current_project: usize,
    /// Attachments for all tasks/projects.
    pub attachments: Vec<Attachment>,
    /// Links for all tasks (appended in task order).
    pub links: Vec<Link>,
    /// Task log entries (`entity_type = 'task'`), appended per task, each
    /// group newest-first (DB order).
    pub logs: Vec<LogEntry>,
    /// Project log entries (`entity_type = 'project'`), appended per
    /// project, each group newest-first (DB order).
    pub project_logs: Vec<LogEntry>,
}

impl TuiData {
    pub fn current(&self) -> Option<&Project> {
        self.projects.get(self.current_project)
    }

    pub fn task(&self, index: usize) -> Option<&Task> {
        self.tasks.get(index)
    }

    /// Tasks belonging to a project (None -> no tasks).
    pub fn tasks_for(&self, project_id: Option<i64>) -> Vec<&Task> {
        match project_id {
            Some(pid) => self
                .tasks
                .iter()
                .filter(|t| t.project_id == Some(pid))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Board columns for a project: (todo, doing, follow-up, blocked, done).
    pub fn board_columns(&self, project_id: i64) -> [Vec<&Task>; 5] {
        let mut cols: [Vec<&Task>; 5] = [vec![], vec![], vec![], vec![], vec![]];
        for t in self
            .tasks
            .iter()
            .filter(|t| t.project_id == Some(project_id))
        {
            match t.status.as_str() {
                "todo" => cols[0].push(t),
                "doing" => cols[1].push(t),
                "follow-up" => cols[2].push(t),
                "blocked" => cols[3].push(t),
                "done" => cols[4].push(t),
                _ => {}
            }
        }
        cols
    }

    /// `4 projects · 16 tasks (5 todo, 2 doing, 1 follow-up, 1 blocked, 7 done)`.
    pub fn summary(&self) -> String {
        let mut counts = [0usize; 5];
        for t in &self.tasks {
            match t.status.as_str() {
                "todo" => counts[0] += 1,
                "doing" => counts[1] += 1,
                "follow-up" => counts[2] += 1,
                "blocked" => counts[3] += 1,
                "done" => counts[4] += 1,
                _ => {}
            }
        }
        format!(
            "{} projects \u{b7} {} tasks ({} todo, {} doing, {} follow-up, {} blocked, {} done)",
            self.projects.len(),
            self.tasks.len(),
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            counts[4]
        )
    }

    pub fn attachments_for_task(&self, task_id: i64) -> Vec<&Attachment> {
        self.attachments
            .iter()
            .filter(|a| a.task_id == Some(task_id))
            .collect()
    }

    pub fn attachments_for_project(&self, project_id: i64) -> Vec<&Attachment> {
        self.attachments
            .iter()
            .filter(|a| a.project_id == Some(project_id))
            .collect()
    }

    pub fn links_for_task(&self, task_id: i64) -> Vec<&Link> {
        self.links
            .iter()
            .filter(|l| l.task_id == Some(task_id))
            .collect()
    }

    /// Log entries for one task, newest first (DB order preserved).
    pub fn logs_for_task(&self, task_id: i64) -> Vec<&LogEntry> {
        self.logs
            .iter()
            .filter(|e| e.entity_id == task_id)
            .collect()
    }

    /// A project's full log: its own entries plus every task entry in the
    /// project, merged newest-first (mirrors `cli::log_view`).
    pub fn project_log_entries(&self, project_id: i64) -> Vec<&LogEntry> {
        let task_ids: Vec<i64> = self
            .tasks
            .iter()
            .filter(|t| t.project_id == Some(project_id))
            .map(|t| t.id)
            .collect();
        let mut all: Vec<&LogEntry> = self
            .project_logs
            .iter()
            .filter(|e| e.entity_id == project_id)
            .collect();
        all.extend(self.logs.iter().filter(|e| task_ids.contains(&e.entity_id)));
        all.sort_by(|a, b| b.id.cmp(&a.id));
        all
    }
}

/// What the log view shows (Phase 6): indices into [`TuiData::tasks`] /
/// [`TuiData::projects`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSubject {
    Task(usize),
    Project(usize),
}

fn is_project_key(s: &str) -> bool {
    let rest = s.strip_prefix("P-").unwrap_or("");
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn is_task_key(s: &str) -> bool {
    let rest = s.strip_prefix("T-").unwrap_or("");
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

/// Resolve a `status <query>` / `log <query>` command-line target against
/// the snapshot (Phase 6), mirroring `cli::resolve_entity`'s precedence:
/// project key, task key, project name, task title — all exact; more than
/// one title match is ambiguous.
pub fn resolve_log_query(data: &TuiData, query: &str) -> Result<LogSubject, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("empty query — try: status <project|task>".to_string());
    }
    if is_project_key(q) {
        return data
            .projects
            .iter()
            .position(|p| p.project_key == q)
            .map(LogSubject::Project)
            .ok_or_else(|| format!("no project with key '{q}'"));
    }
    if is_task_key(q) {
        return data
            .tasks
            .iter()
            .position(|t| t.task_key == q)
            .map(LogSubject::Task)
            .ok_or_else(|| format!("no task with key '{q}'"));
    }
    if let Some(i) = data.projects.iter().position(|p| p.name == q) {
        return Ok(LogSubject::Project(i));
    }
    let hits: Vec<usize> = data
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.title == q)
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [] => Err(format!(
            "unknown project or task '{q}': no exact match (names with whitespace must be typed as-is)"
        )),
        [i] => Ok(LogSubject::Task(*i)),
        many => Err(format!(
            "ambiguous '{q}': {} tasks share this title: {} — use the task key",
            many.len(),
            many
                .iter()
                .map(|&i| data.tasks[i].task_key.clone())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Load a fresh snapshot from the DB; keeps the current project by key
/// (defaults to the first project — the P-000 seed).
pub fn load(
    lun: &Lun,
    version: &str,
    repo_path: &str,
    branch: String,
    current_key: Option<&str>,
) -> Result<TuiData, DbError> {
    let projects = lun.list_projects()?;
    let tasks = lun.list_tasks()?;

    // Phase 6: snapshot everything the task/log views render, so the
    // render path stays DB-free.
    let mut attachments = Vec::new();
    let mut links = Vec::new();
    let mut logs = Vec::new();
    for t in &tasks {
        attachments.extend(lun.attachments_for_task(t.id)?);
        links.extend(lun.links_for_task(t.id)?);
        logs.extend(lun.logs_for("task", t.id)?);
    }
    for p in &projects {
        attachments.extend(lun.attachments_for_project(p.id)?);
    }
    let mut project_logs = Vec::new();
    for p in &projects {
        project_logs.extend(lun.logs_for("project", p.id)?);
    }

    let current_project = current_key
        .and_then(|k| projects.iter().position(|p| p.project_key == k))
        .unwrap_or(0);
    Ok(TuiData {
        version: version.to_string(),
        repo_path: repo_path.to_string(),
        branch,
        projects,
        tasks,
        current_project,
        attachments,
        links,
        logs,
        project_logs,
    })
}
