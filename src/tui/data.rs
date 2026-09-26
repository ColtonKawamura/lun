//! Snapshot of everything the TUI renders, loaded from the DB once per
//! view switch (the TUI is read-only in Phase 5).

use crate::db::{DbError, Lun, Project, Task};

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
}

impl TuiData {
    pub fn current(&self) -> Option<&Project> {
        self.projects.get(self.current_project)
    }

    /// Tasks belonging to a project (None -> no tasks).
    pub fn tasks_for(&self, project_id: Option<i64>) -> Vec<&Task> {
        match project_id {
            Some(pid) => self.tasks.iter().filter(|t| t.project_id == Some(pid)).collect(),
            None => Vec::new(),
        }
    }

    /// Board columns for a project: (todo, in-progress, review, done).
    pub fn board_columns(&self, project_id: i64) -> [Vec<&Task>; 4] {
        let mut cols: [Vec<&Task>; 4] = [vec![], vec![], vec![], vec![]];
        for t in self.tasks.iter().filter(|t| t.project_id == Some(project_id)) {
            match t.status.as_str() {
                "todo" => cols[0].push(t),
                "in-progress" => cols[1].push(t),
                "review" => cols[2].push(t),
                "done" => cols[3].push(t),
                _ => {}
            }
        }
        cols
    }

    /// `4 projects · 16 tasks (5 todo, 2 in-progress, 1 review, 8 done)`.
    pub fn summary(&self) -> String {
        let mut counts = [0usize; 4];
        for t in &self.tasks {
            match t.status.as_str() {
                "todo" => counts[0] += 1,
                "in-progress" => counts[1] += 1,
                "review" => counts[2] += 1,
                "done" => counts[3] += 1,
                _ => {}
            }
        }
        format!(
            "{} projects \u{b7} {} tasks ({} todo, {} in-progress, {} review, {} done)",
            self.projects.len(),
            self.tasks.len(),
            counts[0],
            counts[1],
            counts[2],
            counts[3]
        )
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
    })
}
