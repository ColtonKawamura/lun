//! TUI app state: current view, palette filter/selection, transient
//! message line, and the slash command table.
//!
//! Phase 6 adds vim-style modes on top of the Phase 5 state machine:
//! - [`Mode::Normal`] is the default: `h/j/k/l` move, `/` palette,
//!   `:` statusline, `q` quit, `t` opens the current task, `e` starts
//!   editing the current task's notes.
//! - [`Mode::Insert`] edits the current task's notes buffer; `Esc`
//!   returns to normal mode.
//! - The palette and the statusline have their own key handling too.

use std::path::PathBuf;

use super::data::TuiData;
use crate::db::{LinkTarget, Lun, ProjectSpec, TaskSpec, TaskUpdateSpec};

/// Where the app is looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Initial,
    Status,
    Board,
    Project,
    /// Task detail view (fields, notes, attachments, links, history).
    Task,
    /// Log view for one task or project (newest first).
    Log,
    /// Form to create a task.
    NewTask,
    /// Form to create a project.
    NewProject,
    /// Form to move current task to another project.
    MoveTask,
    Help,
    /// /config — "planned for a later phase".
    Placeholder,
}

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Initial => "Initial",
            View::Status => "Status",
            View::Board => "Board",
            View::Project => "Project",
            View::Task => "Task",
            View::Log => "Log",
            View::NewTask => "New Task",
            View::NewProject => "New Project",
            View::MoveTask => "Move Task",
            View::Help => "Help",
            View::Placeholder => "Coming Soon",
        }
    }
}

/// Vim-style input modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    /// Editing the current task's notes buffer.
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskFocus {
    #[default]
    Summary,
    Notes,
    Attachments,
    Links,
}

const TASK_STATUSES: [&str; 4] = ["todo", "in-progress", "review", "done"];
const TASK_PRIORITIES: [&str; 3] = ["low", "med", "high"];
const PROJECT_STATUSES: [&str; 4] = ["planning", "active", "in-progress", "done"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTaskFormDraft {
    pub field: usize,
    pub title: String,
    pub project_index: usize,
    pub status_index: usize,
    pub priority_index: usize,
    pub assignee: String,
    pub branch: String,
    pub labels: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProjectFormDraft {
    pub field: usize,
    pub name: String,
    pub status_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveTaskFormDraft {
    pub field: usize,
    pub project_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormState {
    NewTask(NewTaskFormDraft),
    NewProject(NewProjectFormDraft),
    MoveTask(MoveTaskFormDraft),
}

/// A slash command in the palette.
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    /// Which view it lands on; `None` = quit. `Some(View::Task)` /
    /// `Some(View::Log)` resolve their subject from the command line
    /// (see [`App::run_command`]).
    pub view: Option<View>,
}

/// The full palette, in the plan's order.
pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/status",
        description: "Show global status (projects + tasks)",
        view: Some(View::Status),
    },
    SlashCommand {
        name: "/board",
        description: "Show kanban board for current project",
        view: Some(View::Board),
    },
    SlashCommand {
        name: "/project",
        description: "Select or view a project",
        view: Some(View::Project),
    },
    SlashCommand {
        name: "/task",
        description: "View a task: /task <T-00N|title> (default: current task)",
        view: Some(View::Task),
    },
    SlashCommand {
        name: "/new-task",
        description: "Create a new task",
        view: Some(View::NewTask),
    },
    SlashCommand {
        name: "/new-project",
        description: "Create a new project",
        view: Some(View::NewProject),
    },
    SlashCommand {
        name: "/move",
        description: "Move current task to a different project",
        view: Some(View::MoveTask),
    },
    SlashCommand {
        name: "/log",
        description: "Show logs: /log <project|task> (default: current project)",
        view: Some(View::Log),
    },
    SlashCommand {
        name: "/config",
        description: "View configuration (planned)",
        view: Some(View::Placeholder),
    },
    SlashCommand {
        name: "/help",
        description: "Show help and keybindings",
        view: Some(View::Help),
    },
    SlashCommand {
        name: "/quit",
        description: "Exit lun",
        view: None,
    },
];

/// The whole TUI application (state + the Phase 7 DB handle).
#[derive(Debug)]
pub struct App {
    pub data: TuiData,
    pub view: View,
    pub previous_view: View,
    pub view_history: Vec<View>,
    pub mode: Mode,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub project_selected: usize,
    /// Task list selection; doubles as the "current task" for the task
    /// view and note editing.
    pub task_selected: usize,
    pub task_focus: TaskFocus,
    pub task_item_selected: usize,
    /// What the log view shows (set by `/log`, `:status <q>`, `/log` default).
    pub log_subject: Option<super::data::LogSubject>,
    /// Active slash-form state (`/new-task`, `/new-project`, `/move`).
    pub form: Option<FormState>,
    /// Statusline: `:` opens it; the remainder of the line is the query.
    pub statusline_open: bool,
    pub statusline_query: String,
    /// Draft buffer for the current task's notes while in insert mode.
    pub notes_draft: String,
    /// Whether the draft differs from the last-committed notes text.
    pub notes_dirty: bool,
    pub pending_g: bool,
    /// (text, is_error) shown on the message line above the hint bar.
    pub message: Option<(String, bool)>,
    pub quit: bool,
    /// The `.lun/` root (Phase 7: the TUI writes through here — notes
    /// saves and drop/paste attachments). `None` for headless test apps
    /// built with [`App::new`].
    pub root: Option<PathBuf>,
    /// Open DB handle for Phase 7 writes (notes, attachments). `None`
    /// for headless test apps.
    pub lun: Option<Lun>,
}

impl App {
    /// Create a fresh app (headless — no DB handle; Phase 7 write paths
    /// no-op with a message). Used by tests and by anything that only
    /// renders/dispatches.
    pub fn new(data: TuiData) -> Self {
        Self {
            data,
            view: View::Initial,
            previous_view: View::Initial,
            view_history: Vec::new(),
            mode: Mode::Normal,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            project_selected: 0,
            task_selected: 0,
            task_focus: TaskFocus::Summary,
            task_item_selected: 0,
            log_subject: None,
            form: None,
            statusline_open: false,
            statusline_query: String::new(),
            notes_draft: String::new(),
            notes_dirty: false,
            pending_g: false,
            message: None,
            quit: false,
            root: None,
            lun: None,
        }
    }

    /// Create the real TUI app (`term::launch`): keeps the open DB and
    /// the `.lun/` root so notes saves and drop/paste attachments work.
    pub fn with_store(data: TuiData, root: PathBuf, lun: Lun) -> Self {
        let mut app = Self::new(data);
        app.root = Some(root);
        app.lun = Some(lun);
        app
    }

    pub fn form(&self) -> Option<&FormState> {
        self.form.as_ref()
    }

    fn begin_new_task_form(&mut self) {
        let project_index = self
            .data
            .current_project
            .min(self.data.projects.len().saturating_sub(1));
        self.form = Some(FormState::NewTask(NewTaskFormDraft {
            field: 0,
            title: String::new(),
            project_index,
            status_index: 0,
            priority_index: 0,
            assignee: "me".to_string(),
            branch: String::new(),
            labels: String::new(),
        }));
    }

    fn begin_new_project_form(&mut self) {
        self.form = Some(FormState::NewProject(NewProjectFormDraft {
            field: 0,
            name: String::new(),
            status_index: 1, // active
        }));
    }

    fn begin_move_task_form(&mut self) -> Result<(), String> {
        let task = self
            .current_task()
            .ok_or_else(|| "no current task to move".to_string())?;
        let project_index = task
            .project_id
            .and_then(|pid| self.data.projects.iter().position(|p| p.id == pid))
            .unwrap_or(self.data.current_project);
        self.form = Some(FormState::MoveTask(MoveTaskFormDraft {
            field: 0,
            project_index,
        }));
        Ok(())
    }

    pub fn form_nav(&mut self, dir: i32) {
        match self.form.as_mut() {
            Some(FormState::NewTask(d)) => {
                let n = 8_i32;
                d.field = ((d.field as i32 + dir).rem_euclid(n)) as usize;
            }
            Some(FormState::NewProject(d)) => {
                let n = 3_i32;
                d.field = ((d.field as i32 + dir).rem_euclid(n)) as usize;
            }
            Some(FormState::MoveTask(d)) => {
                let n = 2_i32;
                d.field = ((d.field as i32 + dir).rem_euclid(n)) as usize;
            }
            None => {}
        }
    }

    pub fn form_type(&mut self, ch: char) {
        match self.form.as_mut() {
            Some(FormState::NewTask(d)) => match d.field {
                0 => d.title.push(ch),
                4 => d.assignee.push(ch),
                5 => d.branch.push(ch),
                6 => d.labels.push(ch),
                _ => {}
            },
            Some(FormState::NewProject(d)) => {
                if d.field == 0 {
                    d.name.push(ch);
                }
            }
            _ => {}
        }
    }

    pub fn form_backspace(&mut self) {
        match self.form.as_mut() {
            Some(FormState::NewTask(d)) => match d.field {
                0 => {
                    d.title.pop();
                }
                4 => {
                    d.assignee.pop();
                }
                5 => {
                    d.branch.pop();
                }
                6 => {
                    d.labels.pop();
                }
                _ => {}
            },
            Some(FormState::NewProject(d)) => {
                if d.field == 0 {
                    d.name.pop();
                }
            }
            _ => {}
        }
    }

    pub fn form_cycle(&mut self, dir: i32) {
        let cycle = |idx: &mut usize, len: usize, dir: i32| {
            *idx = ((*idx as i32 + dir).rem_euclid(len as i32)) as usize;
        };
        match self.form.as_mut() {
            Some(FormState::NewTask(d)) => match d.field {
                1 => cycle(&mut d.project_index, self.data.projects.len().max(1), dir),
                2 => cycle(&mut d.status_index, TASK_STATUSES.len(), dir),
                3 => cycle(&mut d.priority_index, TASK_PRIORITIES.len(), dir),
                _ => {}
            },
            Some(FormState::NewProject(d)) => {
                if d.field == 1 {
                    cycle(&mut d.status_index, PROJECT_STATUSES.len(), dir);
                }
            }
            Some(FormState::MoveTask(d)) => {
                if d.field == 0 {
                    cycle(&mut d.project_index, self.data.projects.len().max(1), dir);
                }
            }
            None => {}
        }
    }

    pub fn cancel_form(&mut self) {
        self.form = None;
        self.go_back();
    }

    pub fn submit_form(&mut self) {
        let Some(form) = self.form.clone() else {
            return;
        };
        match form {
            FormState::NewTask(d) => {
                if d.field != 7 {
                    self.form_nav(1);
                    return;
                }
                let title = d.title.trim().to_string();
                if title.is_empty() {
                    self.message = Some(("title is required".to_string(), true));
                    return;
                }
                let Some(lun) = self.lun.as_ref() else {
                    self.message = Some(("no store attached — create task unavailable".to_string(), true));
                    return;
                };
                let Some(project) = self.data.projects.get(d.project_index) else {
                    self.message = Some(("invalid project selection".to_string(), true));
                    return;
                };
                let project_id = project.id;
                let project_name = project.name.clone();
                let project_key = project.project_key.clone();
                let labels = d
                    .labels
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("\"{}\"", s.replace('"', "\\\"")))
                    .collect::<Vec<_>>();
                let spec = TaskSpec {
                    title: title.clone(),
                    project: Some(project_id),
                    status: Some(TASK_STATUSES[d.status_index].to_string()),
                    priority: Some(TASK_PRIORITIES[d.priority_index].to_string()),
                    assignee: if d.assignee.trim().is_empty() {
                        None
                    } else {
                        Some(d.assignee.trim().to_string())
                    },
                    branch: if d.branch.trim().is_empty() {
                        None
                    } else {
                        Some(d.branch.trim().to_string())
                    },
                    labels: if labels.is_empty() {
                        None
                    } else {
                        Some(format!("[{}]", labels.join(", ")))
                    },
                    message: None,
                    user: None,
                };
                match lun.create_task(spec) {
                    Ok(task) => {
                        let _ = self.refresh_from_store();
                        if let Some(pos) = self.data.tasks.iter().position(|t| t.id == task.id) {
                            self.task_selected = pos;
                        }
                        if let Some(pos) = self.data.projects.iter().position(|p| p.id == project_id) {
                            self.data.current_project = pos;
                            self.project_selected = pos;
                        }
                        self.form = None;
                        self.view = View::Task;
                        self.task_focus = TaskFocus::Summary;
                        self.task_item_selected = 0;
                        self.message = Some((
                            format!(
                                "Created task {} in project {} [{}]",
                                task.task_key, project_name, project_key
                            ),
                            false,
                        ));
                    }
                    Err(e) => self.message = Some((format!("create failed: {e}"), true)),
                }
            }
            FormState::NewProject(d) => {
                if d.field != 2 {
                    self.form_nav(1);
                    return;
                }
                let name = d.name.trim().to_string();
                if name.is_empty() {
                    self.message = Some(("project name is required".to_string(), true));
                    return;
                }
                let Some(lun) = self.lun.as_ref() else {
                    self.message = Some(("no store attached — create project unavailable".to_string(), true));
                    return;
                };
                match lun.create_project(ProjectSpec {
                    name,
                    status: Some(PROJECT_STATUSES[d.status_index].to_string()),
                    message: None,
                    user: None,
                }) {
                    Ok(project) => {
                        let _ = self.refresh_from_store();
                        if let Some(pos) = self.data.projects.iter().position(|p| p.id == project.id) {
                            self.data.current_project = pos;
                            self.project_selected = pos;
                        }
                        self.form = None;
                        self.view = View::Project;
                        self.message = Some((
                            format!("Created project {} [{}]", project.name, project.project_key),
                            false,
                        ));
                    }
                    Err(e) => self.message = Some((format!("create failed: {e}"), true)),
                }
            }
            FormState::MoveTask(d) => {
                if d.field != 1 {
                    self.form_nav(1);
                    return;
                }
                let Some((task_id, task_key, current_pid)) = self
                    .current_task()
                    .map(|t| (t.id, t.task_key.clone(), t.project_id))
                else {
                    self.message = Some(("no current task to move".to_string(), true));
                    return;
                };
                let Some(target) = self.data.projects.get(d.project_index).cloned() else {
                    self.message = Some(("invalid project selection".to_string(), true));
                    return;
                };
                if current_pid == Some(target.id) {
                    self.message = Some((format!("{task_key} already in {}", target.name), false));
                    return;
                }
                let Some(lun) = self.lun.as_ref() else {
                    self.message = Some(("no store attached — move unavailable".to_string(), true));
                    return;
                };
                match lun.update_task(
                    task_id,
                    TaskUpdateSpec {
                        project_id: Some(target.id),
                        message: Some(format!("move {task_key} to {}", target.name)),
                        ..Default::default()
                    },
                ) {
                    Ok(updated) => {
                        let _ = self.refresh_from_store();
                        if let Some(pos) = self.data.tasks.iter().position(|t| t.id == updated.id) {
                            self.task_selected = pos;
                        }
                        if let Some(pos) = self.data.projects.iter().position(|p| p.id == target.id) {
                            self.data.current_project = pos;
                            self.project_selected = pos;
                        }
                        self.form = None;
                        self.view = View::Task;
                        self.message = Some((
                            format!("Moved {} to {} [{}]", task_key, target.name, target.project_key),
                            false,
                        ));
                    }
                    Err(e) => self.message = Some((format!("move failed: {e}"), true)),
                }
            }
        }
    }

    /// The current task (selection clamped into range).
    pub fn current_task(&self) -> Option<&crate::db::Task> {
        if self.data.tasks.is_empty() {
            return None;
        }
        self.data
            .tasks
            .get(self.task_selected.min(self.data.tasks.len() - 1))
    }

    pub fn current_note_links(&self) -> Vec<String> {
        let mut out = Vec::new();
        let notes = if self.mode == Mode::Insert {
            self.notes_draft.as_str()
        } else {
            self.current_task().map(|t| t.notes.as_str()).unwrap_or("")
        };
        for line in notes.lines() {
            let mut rest = line;
            while let Some(label_start) = rest.find('[') {
                let after_label = &rest[label_start + 1..];
                let Some(label_end) = after_label.find("](") else {
                    break;
                };
                let after_open = &after_label[label_end + 2..];
                let Some(uri_end) = after_open.find(')') else {
                    break;
                };
                out.push(after_open[..uri_end].to_string());
                rest = &after_open[uri_end + 1..];
            }
        }
        out
    }

    pub fn current_open_target(&self) -> Option<String> {
        let task = self.current_task()?;
        match self.task_focus {
            TaskFocus::Summary => None,
            TaskFocus::Notes => self
                .current_note_links()
                .get(self.task_item_selected)
                .cloned(),
            TaskFocus::Attachments => self
                .data
                .attachments_for_task(task.id)
                .get(self.task_item_selected)
                .map(|a| a.stored_path.clone()),
            TaskFocus::Links => self
                .data
                .links_for_task(task.id)
                .get(self.task_item_selected)
                .map(|l| l.uri.clone()),
        }
    }

    /// Whether the current task's notes buffer has unsaved edits.
    pub fn notes_modified(&self) -> bool {
        self.mode == Mode::Insert && self.notes_dirty
    }

    fn refresh_from_store(&mut self) -> Result<(), String> {
        let Some(lun) = self.lun.as_ref() else {
            return Ok(());
        };
        let sel = self.task_selected;
        let proj = self.data.current_project;
        let fresh = super::data::load(
            lun,
            &self.data.version,
            &self.data.repo_path,
            self.data.branch.clone(),
            self.data.current().map(|p| p.project_key.as_str()),
        )
        .map_err(|e| e.to_string())?;
        self.data = fresh;
        self.task_selected = sel.min(self.data.tasks.len().saturating_sub(1));
        self.data.current_project = proj.min(self.data.projects.len().saturating_sub(1));
        Ok(())
    }

    /// Palette rows after filtering by the current query.
    ///
    /// The query is typed WITHOUT the leading `/` (the prompt shows `› /<query>`),
    /// so compare against the command name minus its slash. Phase 6: the
    /// query may carry a command line after the command word (e.g.
    /// `task T-001`); only the word before the first space filters.
    pub fn filtered_commands(&self) -> Vec<&'static SlashCommand> {
        let prefix = self
            .palette_query
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        SLASH_COMMANDS
            .iter()
            .filter(|c| c.name.trim_start_matches('/').starts_with(&prefix))
            .collect()
    }

    /// Execute the selected (or given) slash command.
    pub fn run_command(&mut self, index: usize) {
        let filtered = self.filtered_commands();
        let Some(cmd) = filtered.get(index) else {
            let query = self.palette_query.clone();
            self.close_palette();
            self.message = Some((format!("no command matches '{query}'"), true));
            return;
        };
        // Anything after the command name in the palette query is the
        // command line (e.g. `/task T-002` or `/log paper-stack`).
        let rest = {
            let rest = self.palette_query.as_str();
            let name = cmd.name.trim_start_matches('/');
            if rest.len() > name.len() {
                let after = &rest[name.len()..];
                if after.starts_with(char::is_whitespace) {
                    after.trim_start().to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };
        match cmd.view {
            None => {
                self.palette_open = false;
                self.palette_query.clear();
                self.palette_selected = 0;
                // `q` guards unsaved notes like quit does.
                if self.notes_modified() {
                    self.message = Some((
                        "unsaved note edits — press Esc in insert mode first".to_string(),
                        true,
                    ));
                    return;
                }
                self.quit = true;
                return;
            }
            Some(view) => {
                self.enter_view(view, &rest);
            }
        }
    }

    /// Switch views, resolving task/log subjects. `rest` is the command
    /// line text after the command name.
    pub fn enter_view(&mut self, view: View, rest: &str) {
        self.close_palette();
        if self.notes_modified() {
            self.message = Some((
                "unsaved note edits — press Esc in insert mode first".to_string(),
                true,
            ));
            return;
        }
        self.mode = Mode::Normal;
        self.pending_g = false;
        self.statusline_open = false;
        self.statusline_query.clear();
        self.message = None;
        if self.view != view {
            self.view_history.push(self.view);
        }
        self.previous_view = self.view;
        match view {
            View::Task => {
                if rest.is_empty() {
                    if self.current_task().is_none() {
                        self.message = Some(("no tasks to show".to_string(), true));
                        return;
                    }
                } else if let Err(e) = self.select_task_query(rest) {
                    self.message = Some((e, true));
                    return;
                }
                self.task_focus = TaskFocus::Summary;
                self.task_item_selected = 0;
                self.view = View::Task;
            }
            View::Log => {
                if rest.is_empty() {
                    // Default: the current project's log.
                    self.log_subject =
                        Some(super::data::LogSubject::Project(self.data.current_project));
                } else {
                    match super::data::resolve_log_query(&self.data, rest) {
                        Ok(subject) => self.log_subject = Some(subject),
                        Err(e) => {
                            self.message = Some((e, true));
                            return;
                        }
                    }
                }
                self.view = View::Log;
            }
            View::NewTask => {
                self.begin_new_task_form();
                if let Some(FormState::NewTask(d)) = self.form.as_mut() {
                    if !rest.trim().is_empty() {
                        d.title = rest.trim().to_string();
                    }
                }
                self.view = View::NewTask;
            }
            View::NewProject => {
                self.begin_new_project_form();
                self.view = View::NewProject;
            }
            View::MoveTask => match self.begin_move_task_form() {
                Ok(()) => self.view = View::MoveTask,
                Err(e) => self.message = Some((e, true)),
            },
            _ => self.view = view,
        }
    }

    /// Select a task from a key or exact title; sets `task_selected`.
    pub fn select_task_query(&mut self, query: &str) -> Result<(), String> {
        let q = query.trim();
        if q.is_empty() {
            return Err("empty task query".to_string());
        }
        if q.starts_with("T-") {
            let pos = self
                .data
                .tasks
                .iter()
                .position(|t| t.task_key == q)
                .ok_or_else(|| format!("no task with key '{q}'"))?;
            self.task_selected = pos;
            return Ok(());
        }
        let hits: Vec<usize> = self
            .data
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.title == q)
            .map(|(i, _)| i)
            .collect();
        match hits.as_slice() {
            [] => Err(format!(
                "no task has key or title '{q}' (try `:status` to list)"
            )),
            [i] => {
                self.task_selected = *i;
                Ok(())
            }
            many => Err(format!(
                "ambiguous task '{q}': {} share this title: {} — use the task key",
                many.len(),
                many.iter()
                    .map(|&i| self.data.tasks[i].task_key.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// Open the palette (reset state).
    pub fn open_palette(&mut self) {
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_selected = 0;
    }

    fn close_palette(&mut self) {
        self.palette_open = false;
        self.palette_query.clear();
        self.palette_selected = 0;
    }

    /// Append a typed character to the palette query; clamp selection.
    pub fn palette_type(&mut self, ch: char) {
        self.palette_query.push(ch);
        let n = self.filtered_commands().len();
        if n == 0 {
            self.palette_selected = 0;
        } else if self.palette_selected >= n {
            self.palette_selected = n - 1;
        }
    }

    pub fn palette_backspace(&mut self) {
        self.palette_query.pop();
        let n = self.filtered_commands().len();
        if n == 0 {
            self.palette_selected = 0;
        } else if self.palette_selected >= n {
            self.palette_selected = n - 1;
        }
    }

    pub fn palette_up(&mut self) {
        let n = self.filtered_commands().len();
        if n == 0 {
            return;
        }
        self.palette_selected = if self.palette_selected == 0 {
            n - 1
        } else {
            self.palette_selected - 1
        };
    }

    pub fn palette_down(&mut self) {
        let n = self.filtered_commands().len();
        if n == 0 {
            return;
        }
        self.palette_selected = (self.palette_selected + 1) % n;
    }

    /// j/k navigation in the `/project` view; Enter selects.
    pub fn project_nav(&mut self, dir: i32, select: bool) {
        if self.data.projects.is_empty() {
            return;
        }
        self.pending_g = false;
        let n = self.data.projects.len();
        self.project_selected =
            ((self.project_selected as i32 + dir).rem_euclid(n as i32)) as usize;
        if select {
            self.data.current_project = self.project_selected;
            self.message = Some((
                format!(
                    "current project: {} [{}]",
                    self.data.projects[self.project_selected].name,
                    self.data.projects[self.project_selected].project_key
                ),
                false,
            ));
        }
    }

    /// j/k task-list navigation (board/status/task views); wraps around.
    pub fn task_nav(&mut self, dir: i32) {
        if self.data.tasks.is_empty() {
            return;
        }
        self.pending_g = false;
        let n = self.data.tasks.len();
        self.task_selected = ((self.task_selected as i32 + dir).rem_euclid(n as i32)) as usize;
    }

    pub fn jump_top(&mut self) {
        self.pending_g = false;
        match self.view {
            View::Project => self.project_selected = 0,
            _ => self.task_selected = 0,
        }
    }

    pub fn jump_bottom(&mut self) {
        self.pending_g = false;
        match self.view {
            View::Project => {
                self.project_selected = self.data.projects.len().saturating_sub(1);
            }
            _ => {
                self.task_selected = self.data.tasks.len().saturating_sub(1);
            }
        }
    }

    pub fn page_nav(&mut self, dir: i32) {
        match self.view {
            View::Project => self.project_nav(dir * 5, false),
            _ => self.task_nav(dir * 5),
        }
    }

    pub fn task_focus_next(&mut self) {
        self.pending_g = false;
        self.task_item_selected = 0;
        self.task_focus = match self.task_focus {
            TaskFocus::Summary => TaskFocus::Notes,
            TaskFocus::Notes => TaskFocus::Attachments,
            TaskFocus::Attachments => TaskFocus::Links,
            TaskFocus::Links => TaskFocus::Summary,
        };
    }

    pub fn task_focus_prev(&mut self) {
        self.pending_g = false;
        self.task_item_selected = 0;
        self.task_focus = match self.task_focus {
            TaskFocus::Summary => TaskFocus::Links,
            TaskFocus::Notes => TaskFocus::Summary,
            TaskFocus::Attachments => TaskFocus::Notes,
            TaskFocus::Links => TaskFocus::Attachments,
        };
    }

    pub fn task_item_nav(&mut self, dir: i32) -> bool {
        let len = match (self.current_task(), self.task_focus) {
            (Some(_), TaskFocus::Summary) => 0,
            (Some(_), TaskFocus::Notes) => self.current_note_links().len(),
            (Some(task), TaskFocus::Attachments) => self.data.attachments_for_task(task.id).len(),
            (Some(task), TaskFocus::Links) => self.data.links_for_task(task.id).len(),
            _ => 0,
        };
        if len == 0 {
            return false;
        }
        self.task_item_selected =
            ((self.task_item_selected as i32 + dir).rem_euclid(len as i32)) as usize;
        true
    }

    pub fn go_back(&mut self) {
        if self.notes_modified() {
            self.message = Some((
                "unsaved note edits — press Esc in insert mode first".to_string(),
                true,
            ));
            return;
        }
        if self.view == View::Task && self.task_focus != TaskFocus::Summary {
            self.task_focus = TaskFocus::Summary;
            self.task_item_selected = 0;
            return;
        }
        if self.view != View::Initial {
            if let Some(prev) = self.view_history.pop() {
                self.previous_view = self.view;
                self.view = prev;
            }
        }
    }

    pub fn open_current_item(&mut self) {
        let Some(target) = self.current_open_target() else {
            self.message = Some(("nothing openable is selected".to_string(), true));
            return;
        };
        match crate::cli::open_target(&target) {
            Ok(()) => self.message = Some((format!("Opened: {target}"), false)),
            Err(e) => self.message = Some((format!("open failed: {e}"), true)),
        }
    }

    pub fn toggle_complete_current_task(&mut self) {
        let Some((task_id, task_key, done)) = self
            .current_task()
            .map(|t| (t.id, t.task_key.clone(), t.status == "done"))
        else {
            self.message = Some(("no current task".to_string(), true));
            return;
        };
        let Some(lun) = self.lun.as_ref() else {
            self.message = Some((
                "no store attached — completion unavailable".to_string(),
                true,
            ));
            return;
        };
        let result = if done {
            lun.reopen_task(task_id, Some("in-progress"), None, None)
        } else {
            lun.complete_task(task_id, None, None)
        };
        match result {
            Ok(_) => {
                let _ = self.refresh_from_store();
                self.message = Some((
                    format!(
                        "Committed: {} {}",
                        if done { "reopen" } else { "complete" },
                        task_key
                    ),
                    false,
                ));
            }
            Err(e) => self.message = Some((format!("task update failed: {e}"), true)),
        }
    }

    /// `t` in normal mode: jump to the current task's detail view.
    pub fn open_current_task(&mut self) {
        if self.current_task().is_none() {
            self.message = Some(("no tasks to show".to_string(), true));
            return;
        }
        if self.notes_modified() {
            self.message = Some((
                "unsaved note edits — press Esc in insert mode first".to_string(),
                true,
            ));
            return;
        }
        self.mode = Mode::Normal;
        self.previous_view = self.view;
        self.view_history.push(self.view);
        self.statusline_open = false;
        self.statusline_query.clear();
        self.task_focus = TaskFocus::Summary;
        self.task_item_selected = 0;
        self.view = View::Task;
    }

    /// `e` in normal mode (task view): start editing the current task's
    /// notes. The buffer starts from the last-committed notes text.
    pub fn enter_notes_edit(&mut self) {
        if self.current_task().is_none() || self.view != View::Task {
            return;
        }
        if self.mode == Mode::Insert && self.notes_dirty {
            return; // already editing
        }
        self.mode = Mode::Insert;
        self.notes_draft = self
            .current_task()
            .map(|t| t.notes.clone())
            .unwrap_or_default();
        self.notes_dirty = false;
        self.statusline_open = false;
        self.statusline_query.clear();
        self.message = Some((
            "note: editing notes — Esc to finish & save, Ctrl-S to save now".to_string(),
            false,
        ));
    }

    /// Insert-mode editing of the notes draft.
    pub fn notes_type(&mut self, ch: char) {
        if self.mode != Mode::Insert {
            return;
        }
        self.notes_draft.push(ch);
        self.notes_dirty = true;
    }

    pub fn notes_newline(&mut self) {
        if self.mode != Mode::Insert {
            return;
        }
        self.notes_draft.push('\n');
        self.notes_dirty = true;
    }

    pub fn notes_backspace(&mut self) {
        if self.mode != Mode::Insert {
            return;
        }
        self.notes_draft.pop();
        self.notes_dirty = true;
    }

    /// `Esc` in insert mode: back to normal mode. Phase 7: Esc is "save
    /// and done" when the draft changed (vim-style write-out) — no silent
    /// data loss; an unchanged draft exits without a log entry.
    pub fn exit_insert(&mut self) {
        if self.mode != Mode::Insert {
            return;
        }
        self.mode = Mode::Normal;
        if self.notes_dirty {
            // Esc = save-and-exit: the default commit message applies
            // (`update notes for <task>`).
            self.save_notes_draft(None);
        } else if !self.notes_draft.is_empty() {
            self.message = Some(("notes unchanged — nothing to save".to_string(), false));
        }
    }

    /// Persist the notes draft to the DB (log-on-write: `UPDATE` entry)
    /// and refresh the snapshot. Called by Esc (save-and-exit) and
    /// Ctrl-S (save-and-stay); `message` is the commit message for the
    /// log entry (default: `update notes for <task>`).
    pub fn save_notes_draft(&mut self, message: Option<&str>) {
        // Copy the fields we need out of the (immutable) current-task
        // borrow BEFORE taking the mutable `self.lun` borrow, so the two
        // borrows never overlap.
        let Some((task_id, task_key)) = self.current_task().map(|t| (t.id, t.task_key.clone()))
        else {
            self.notes_dirty = false;
            return;
        };
        let Some(lun) = self.lun.as_ref() else {
            // Headless app (tests): no store to write through.
            self.message = Some((
                "no store attached — notes cannot be saved".to_string(),
                true,
            ));
            return;
        };
        let notes = self.notes_draft.trim_end().to_string();
        let result = lun.set_notes(task_id, &notes, message, None);
        match result {
            Ok(()) => {
                self.notes_dirty = false;
                self.notes_draft = notes.clone();
                // Refresh the snapshot so the rendered fields/history are
                // current; fall back to patching the task row in place.
                let committed = match message {
                    Some(m) => m.to_string(),
                    None => format!("update notes for {task_key}"),
                };
                if let Ok(fresh) = super::data::load(
                    lun,
                    &self.data.version,
                    &self.data.repo_path,
                    self.data.branch.clone(),
                    self.data
                        .current()
                        .map(|p| p.project_key.clone())
                        .as_deref(),
                ) {
                    let sel = self.task_selected;
                    let proj = self.data.current_project;
                    self.data = fresh;
                    self.task_selected = sel;
                    self.data.current_project = proj;
                } else {
                    if let Some(t) = self.data.tasks.get_mut(self.task_selected) {
                        t.notes = notes.clone();
                    }
                }
                self.message = Some((format!("Committed: {committed}"), false));
            }
            Err(e) => {
                self.message = Some((format!("save failed: {e}"), true));
            }
        }
    }

    /// Drop/paste a file into the TUI (Phase 7, docs/plan.md): the
    /// terminal delivers the dropped path as crossterm `Event::Paste`.
    /// Existing local file paths (quoted, escaped, and/or `file://` URI)
    /// are turned into markdown links with `file:///...` URIs and stored
    /// via `links` rows (no copy into `.lun/attachments/`).
    pub fn attach_dropped_file(&mut self, text: &str) {
        let paths = Self::parse_dropped_paths(text)
            .into_iter()
            .filter(|p| p.is_file())
            .collect::<Vec<_>>();
        if paths.is_empty() {
            self.message = Some((
                "drop one or more existing file paths to link them".to_string(),
                true,
            ));
            return;
        }
        let Some(lun) = self.lun.as_ref() else {
            self.message = Some((
                "no store attached — drop/paste linking is unavailable".to_string(),
                true,
            ));
            return;
        };

        if let Some(task) = self.current_task() {
            let task_id = task.id;
            let task_key = task.task_key.clone();
            let mut notes = if self.mode == Mode::Insert {
                self.notes_draft.trim_end().to_string()
            } else {
                task.notes.trim_end().to_string()
            };
            {
                for path in &paths {
                    let filename = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let uri = Self::file_uri_from_path(path);
                    if let Err(e) = lun.add_link(LinkTarget::Task(task_id), &filename, &uri, None, None)
                    {
                        self.message = Some((format!("link failed: {e}"), true));
                        return;
                    }
                    if !notes.is_empty() {
                        notes.push('\n');
                    }
                    notes.push_str(&format!("- [{filename}]({uri})"));
                }
            }
            self.notes_draft = notes;
            self.notes_dirty = true;
            self.save_notes_draft(None);
            self.message = Some((
                format!("Linked {} file path(s) on {task_key}", paths.len()),
                false,
            ));
            return;
        }

        let Some(project) = self.data.current().cloned() else {
            self.message = Some(("no current project to link files onto".to_string(), true));
            return;
        };
        {
            for path in &paths {
                let filename = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let uri = Self::file_uri_from_path(path);
                if let Err(e) =
                    lun.add_link(LinkTarget::Project(project.id), &filename, &uri, None, None)
                {
                    self.message = Some((format!("link failed: {e}"), true));
                    return;
                }
            }
        }
        let _ = self.refresh_from_store();
        self.message = Some((
            format!("Linked {} file path(s) on project {}", paths.len(), project.project_key),
            false,
        ));
    }

    /// `:status <query>` — run the statusline query against the log
    /// resolver; success switches to the log view, failure stays put with
    /// an error message.
    pub fn run_statusline(&mut self) {
        let q = self.statusline_query.trim().to_string();
        self.statusline_open = false;
        self.statusline_query.clear();
        let (cmd, rest) = match q.split_once(' ') {
            Some((c, r)) if !c.is_empty() => (c, r),
            Some((c, "")) => (c, ""),
            _ => ("status", q.as_str()),
        };
        if cmd != "status" {
            self.message = Some((
                format!("unknown quick action '{cmd}' (Phase 6: status)"),
                true,
            ));
            return;
        }
        match super::data::resolve_log_query(&self.data, rest) {
            Ok(subject) => {
                self.log_subject = Some(subject);
                self.view = View::Log;
                self.mode = Mode::Normal;
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn decode_file_uri(uri: &str) -> Option<String> {
        let rest = uri.strip_prefix("file://")?;
        let path = if let Some(r) = rest.strip_prefix('/') {
            format!("/{r}")
        } else if let Some(r) = rest.strip_prefix("localhost/") {
            format!("/{r}")
        } else {
            return None;
        };
        let mut bytes = Vec::with_capacity(path.len());
        let mut i = 0;
        let b = path.as_bytes();
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                let hex = &path[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    bytes.push(v);
                    i += 3;
                    continue;
                }
            }
            bytes.push(b[i]);
            i += 1;
        }
        String::from_utf8(bytes).ok()
    }

    fn split_shell_like(input: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_single = false;
        let mut in_double = false;
        let mut chars = input.trim().chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => {
                    if let Some(next) = chars.next() {
                        cur.push(next);
                    }
                }
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                c if c.is_whitespace() && !in_single && !in_double => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                c => cur.push(c),
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }

    fn file_uri_from_path(path: &std::path::Path) -> String {
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let raw = abs.to_string_lossy();
        let mut encoded = String::with_capacity(raw.len() + 8);
        for b in raw.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                    encoded.push(b as char)
                }
                _ => encoded.push_str(&format!("%{b:02X}")),
            }
        }
        format!("file://{encoded}")
    }

    pub(crate) fn parse_dropped_paths(text: &str) -> Vec<std::path::PathBuf> {
        Self::split_shell_like(text)
            .into_iter()
            .filter_map(|token| {
                if token.trim().is_empty() {
                    return None;
                }
                let decoded = Self::decode_file_uri(&token).unwrap_or(token);
                Some(std::path::PathBuf::from(decoded))
            })
            .collect()
    }
}
