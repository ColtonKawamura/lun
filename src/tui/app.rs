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
use std::time::{Duration, Instant};
use std::{io::BufReader, io::Cursor};

use super::data::TuiData;
use crate::db::{LinkTarget, Lun, ProjectSpec, TaskSpec, TaskUpdateSpec};

/// Where the app is looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Initial,
    Output,
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
            View::Output => "Output",
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

const TASK_STATUSES: [&str; 5] = ["todo", "doing", "follow-up", "blocked", "done"];
const TASK_PRIORITIES: [&str; 3] = ["low", "med", "high"];
const PROJECT_STATUSES: [&str; 2] = ["active", "inactive"];
const FORM_ESC_CANCEL_WINDOW: Duration = Duration::from_millis(450);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub command: String,
    pub text: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSession {
    pub args: Vec<String>,
    pub prompts: Vec<String>,
    pub answers: Vec<String>,
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
        name: "/new",
        description: "Create a new item: /new task|proj",
        view: Some(View::Placeholder),
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
    pub forward_view_history: Vec<View>,
    pub mode: Mode,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub palette_vim_nav: bool,
    pub command_history: Vec<String>,
    pub history_index: Option<usize>,
    pub history_draft: Option<String>,
    pub output: Option<CommandOutput>,
    pub output_scroll: usize,
    pub output_page_rows: usize,
    pub prompt_session: Option<PromptSession>,
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
    pub form_vim_nav: bool,
    pub form_esc_armed_at: Option<Instant>,
    /// Statusline: `:` opens it; the remainder of the line is the query.
    pub statusline_open: bool,
    pub statusline_query: String,
    /// Draft buffer for the current task's notes while in insert mode.
    pub notes_draft: String,
    /// Whether the draft differs from the last-committed notes text.
    pub notes_dirty: bool,
    pub pending_g: bool,
    pub pending_space: bool,
    pub pending_space_f: bool,
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
            forward_view_history: Vec::new(),
            mode: Mode::Normal,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            palette_vim_nav: false,
            command_history: Vec::new(),
            history_index: None,
            history_draft: None,
            output: None,
            output_scroll: 0,
            output_page_rows: 5,
            prompt_session: None,
            project_selected: 0,
            task_selected: 0,
            task_focus: TaskFocus::Summary,
            task_item_selected: 0,
            log_subject: None,
            form: None,
            form_vim_nav: false,
            form_esc_armed_at: None,
            statusline_open: false,
            statusline_query: String::new(),
            notes_draft: String::new(),
            notes_dirty: false,
            pending_g: false,
            pending_space: false,
            pending_space_f: false,
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
        self.form_vim_nav = false;
        self.form_esc_armed_at = None;
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
        self.form_vim_nav = false;
        self.form_esc_armed_at = None;
        self.form = Some(FormState::NewProject(NewProjectFormDraft {
            field: 0,
            name: String::new(),
            status_index: 0, // active
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
        self.form_vim_nav = false;
        self.form_esc_armed_at = None;
        self.form = Some(FormState::MoveTask(MoveTaskFormDraft {
            field: 0,
            project_index,
        }));
        Ok(())
    }

    pub fn form_nav(&mut self, dir: i32) {
        self.form_esc_armed_at = None;
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
        self.form_vim_nav = false;
        self.form_esc_armed_at = None;
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
        self.form_esc_armed_at = None;
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
        self.form_esc_armed_at = None;
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
        self.form_vim_nav = false;
        self.form_esc_armed_at = None;
        self.go_back();
    }

    pub fn form_escape(&mut self) {
        if self.form.is_none() {
            return;
        }
        let now = Instant::now();
        let quick_second = self
            .form_esc_armed_at
            .is_some_and(|armed| now.duration_since(armed) <= FORM_ESC_CANCEL_WINDOW);
        if quick_second {
            self.cancel_form();
            return;
        }
        self.form_vim_nav = true;
        self.form_esc_armed_at = Some(now);
    }

    pub fn submit_form(&mut self) {
        self.form_esc_armed_at = None;
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
                    self.message = Some((
                        "no store attached — create task unavailable".to_string(),
                        true,
                    ));
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
                        if let Some(pos) =
                            self.data.projects.iter().position(|p| p.id == project_id)
                        {
                            self.data.current_project = pos;
                            self.project_selected = pos;
                        }
                        self.form = None;
                        self.form_vim_nav = false;
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
                    self.message = Some((
                        "no store attached — create project unavailable".to_string(),
                        true,
                    ));
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
                        if let Some(pos) =
                            self.data.projects.iter().position(|p| p.id == project.id)
                        {
                            self.data.current_project = pos;
                            self.project_selected = pos;
                        }
                        self.form = None;
                        self.form_vim_nav = false;
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
                        if let Some(pos) = self.data.projects.iter().position(|p| p.id == target.id)
                        {
                            self.data.current_project = pos;
                            self.project_selected = pos;
                        }
                        self.form = None;
                        self.form_vim_nav = false;
                        self.view = View::Task;
                        self.message = Some((
                            format!(
                                "Moved {} to {} [{}]",
                                task_key, target.name, target.project_key
                            ),
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

    pub fn current_command_prompt(&self) -> String {
        if let Some(prompt) = self.current_prompt_label() {
            prompt
        } else {
            format!("/{}", self.palette_query)
        }
    }

    pub fn current_prompt_label(&self) -> Option<String> {
        self.prompt_session
            .as_ref()
            .and_then(|s| s.prompts.get(s.answers.len()).cloned())
    }

    fn completion_app(&self) -> Option<crate::cli::App> {
        self.root
            .as_deref()
            .and_then(|root| crate::cli::App::open(root).ok())
    }

    pub fn command_suggestions(&self) -> Vec<String> {
        if self.prompt_session.is_some() {
            return Vec::new();
        }
        let app = self.completion_app();
        crate::cli::complete_line(app.as_ref(), &self.palette_query)
    }

    pub fn filtered_commands(&self) -> Vec<String> {
        self.command_suggestions()
    }

    fn quote_completion(value: &str) -> String {
        let has_ws = value.chars().any(|c| c.is_whitespace());
        if value.is_empty() || (!has_ws && !value.contains('"') && !value.contains('\\')) {
            return value.to_string();
        }
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }

    pub fn apply_selected_suggestion(&mut self) {
        self.stop_history_navigation();
        let suggestions = self.command_suggestions();
        let Some(suggestion) = suggestions.get(self.palette_selected).cloned() else {
            return;
        };
        let mut words =
            crate::cli::split_command_line(&self.palette_query).unwrap_or_else(|_| Vec::new());
        let trailing_ws = self
            .palette_query
            .chars()
            .last()
            .map(|c| c.is_whitespace())
            .unwrap_or(false);
        let suggestion = Self::quote_completion(&suggestion);
        if trailing_ws || words.is_empty() {
            words.push(suggestion);
        } else if let Some(last) = words.last_mut() {
            *last = suggestion;
        }
        self.palette_query = words.join(" ");
        self.palette_vim_nav = false;
    }

    fn record_command_history(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if self.command_history.last().map(String::as_str) == Some(line) {
            return;
        }
        self.command_history.push(line.to_string());
    }

    fn stop_history_navigation(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }

    pub fn history_prev(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        match self.history_index {
            Some(0) => {}
            Some(i) => {
                self.history_index = Some(i - 1);
                self.palette_query = self.command_history[i - 1].clone();
            }
            None => {
                self.history_draft = Some(self.palette_query.clone());
                let i = self.command_history.len() - 1;
                self.history_index = Some(i);
                self.palette_query = self.command_history[i].clone();
            }
        }
        self.palette_selected = 0;
        self.palette_vim_nav = false;
    }

    pub fn history_next(&mut self) {
        let Some(i) = self.history_index else {
            return;
        };
        if i + 1 < self.command_history.len() {
            self.history_index = Some(i + 1);
            self.palette_query = self.command_history[i + 1].clone();
        } else {
            self.palette_query = self.history_draft.take().unwrap_or_default();
            self.history_index = None;
        }
        self.palette_selected = 0;
        self.palette_vim_nav = false;
    }

    fn current_context_project_key(&self) -> Option<String> {
        if self.view == View::Task {
            if let Some(pid) = self.current_task().and_then(|t| t.project_id) {
                if let Some(project) = self.data.projects.iter().find(|p| p.id == pid) {
                    return Some(project.project_key.clone());
                }
            }
        }
        self.data.current().map(|p| p.project_key.clone())
    }

    fn normalize_command_args(&self, args: Vec<String>) -> Vec<String> {
        let mut args = args;
        if args.is_empty() {
            return args;
        }
        match args.first().map(String::as_str) {
            Some("task") if args.len() == 1 => {
                if let Some(task) = self.current_task() {
                    args.push(task.task_key.clone());
                }
            }
            Some("log") if args.len() == 1 => {
                if self.view == View::Task {
                    if let Some(task) = self.current_task() {
                        args.push(task.task_key.clone());
                    }
                } else if let Some(project) = self.data.current() {
                    args.push(project.project_key.clone());
                }
            }
            Some("status") if args.len() == 2 && args[1] == "--board" => {
                if let Some(project) = self.data.current() {
                    args.insert(1, project.project_key.clone());
                }
            }
            Some("add")
                if args.get(1).map(String::as_str) == Some("task")
                    && !args.iter().any(|a| a == "proj" || a == "project") =>
            {
                if let Some(project_key) = self.current_context_project_key() {
                    args.push("proj".to_string());
                    args.push(project_key);
                }
            }
            _ => {}
        }
        args
    }

    fn prompt_labels_for_command(&self, args: &[String]) -> Result<Vec<String>, String> {
        let mut prompts = Vec::new();
        match args.first().map(String::as_str) {
            Some("add") if args.get(1).map(String::as_str) == Some("task") => {
                prompts.extend([
                    "Status? (todo, doing, follow-up, blocked, done): ".to_string(),
                    "Priority? (low, med, or high): ".to_string(),
                    "Assignee? (default: me): ".to_string(),
                    "Commit Message: ".to_string(),
                ]);
            }
            Some("add") if matches!(args.get(1).map(String::as_str), Some("proj" | "project")) => {
                if !args.iter().any(|a| a == "--message") {
                    prompts.push("Commit Message: ".to_string());
                }
            }
            Some("new") if matches!(args.get(1).map(String::as_str), Some("proj" | "project")) => {
                if !args.iter().any(|a| a == "--message") {
                    prompts.push("Commit Message: ".to_string());
                }
            }
            Some("move") if !args.iter().any(|a| a == "--message") => {
                prompts.push("Commit Message: ".to_string());
            }
            Some("task") => match args.get(1).map(String::as_str) {
                Some("edit") if !args.iter().any(|a| a == "--message") => {
                    prompts.push("Commit Message: ".to_string());
                }
                Some("complete") | Some("archive") | Some("delete") => {
                    prompts.push("Commit Message: ".to_string());
                }
                Some("reopen") => prompts.push("Commit Message: ".to_string()),
                Some(_)
                    if args.iter().any(|a| a == "--status")
                        && !args.iter().any(|a| a == "--message") =>
                {
                    prompts.push("Commit Message: ".to_string());
                }
                _ => {}
            },
            Some("proj") | Some("project")
                if args.iter().any(|a| a == "--status")
                    && !args.iter().any(|a| a == "--message") =>
            {
                prompts.push("Commit Message: ".to_string());
            }
            Some("attach")
                if matches!(args.get(1).map(String::as_str), Some("task" | "project")) =>
            {
                let Some(root) = self.root.as_deref() else {
                    return Ok(prompts);
                };
                let Some(file) = args.get(3) else {
                    return Ok(prompts);
                };
                let src = std::path::Path::new(file);
                let inside = std::path::absolute(src)
                    .ok()
                    .zip(std::path::absolute(root).ok())
                    .map(|(s, r)| s.starts_with(&r))
                    .unwrap_or(false);
                if !inside {
                    prompts.push(
                        "This path is outside the current repo. Link anyway? [y/N] ".to_string(),
                    );
                }
            }
            _ => {}
        }
        Ok(prompts)
    }

    fn select_context_from_args(&mut self, cli_app: &crate::cli::App, args: &[String]) {
        match args.first().map(String::as_str) {
            Some("task") => {
                let query = match args.get(1).map(String::as_str) {
                    Some("ls" | "edit" | "complete" | "reopen" | "archive" | "delete") => {
                        args.get(2).map(String::as_str)
                    }
                    other => other,
                };
                if let Some(query) = query {
                    if let Ok(task) = crate::cli::resolve_task(&cli_app.lun, query) {
                        if let Some(pos) = self.data.tasks.iter().position(|t| t.id == task.id) {
                            self.task_selected = pos;
                        }
                    }
                }
            }
            Some("status") | Some("log") => {
                if let Some(query) = args.get(1) {
                    match crate::cli::resolve_entity(&cli_app.lun, query) {
                        Ok(crate::cli::Entity::Project(project)) => {
                            if let Some(pos) =
                                self.data.projects.iter().position(|p| p.id == project.id)
                            {
                                self.data.current_project = pos;
                                self.project_selected = pos;
                            }
                        }
                        Ok(crate::cli::Entity::Task(task)) => {
                            if let Some(pos) = self.data.tasks.iter().position(|t| t.id == task.id)
                            {
                                self.task_selected = pos;
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            Some("proj") | Some("project") => {
                if let Some(query) = args.get(1) {
                    if let Ok(project) = crate::cli::resolve_project(&cli_app.lun, query) {
                        if let Some(pos) =
                            self.data.projects.iter().position(|p| p.id == project.id)
                        {
                            self.data.current_project = pos;
                            self.project_selected = pos;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn execute_cli_args(&mut self, args: Vec<String>, command: String) {
        if self.try_open_native_command(&args) {
            return;
        }
        let Some(root) = self.root.as_deref() else {
            self.close_palette();
            self.message = Some((
                "no store attached — command execution unavailable".to_string(),
                true,
            ));
            return;
        };
        let cli_app = match crate::cli::App::open(root) {
            Ok(app) => app,
            Err(e) => {
                self.close_palette();
                self.message = Some((format!("opening command app failed: {e}"), true));
                return;
            }
        };
        let prompts = match self.prompt_labels_for_command(&args) {
            Ok(prompts) => prompts,
            Err(e) => {
                self.close_palette();
                self.message = Some((e, true));
                return;
            }
        };
        if !prompts.is_empty() {
            self.prompt_session = Some(PromptSession {
                args,
                prompts,
                answers: Vec::new(),
            });
            self.palette_query.clear();
            self.palette_selected = 0;
            if self.output.is_some() {
                self.view = View::Output;
            }
            self.message = None;
            return;
        }
        let mut reader = BufReader::new(Cursor::new(Vec::<u8>::new()));
        let result = crate::cli::run_result_in_reader(&cli_app, &args, &mut reader, Some(root));
        self.finish_command(command, &cli_app, &args, result);
    }

    fn try_open_native_command(&mut self, args: &[String]) -> bool {
        let [cmd, subcmd, title, rest @ ..] = args else {
            return false;
        };
        if cmd != "add" || subcmd != "task" {
            return false;
        }
        let project_id = match rest {
            [] => None,
            [kind, query] if kind == "proj" || kind == "project" => {
                let Some(lun) = self.lun.as_ref() else {
                    self.message = Some((
                        "no store attached — create task unavailable".to_string(),
                        true,
                    ));
                    return true;
                };
                match crate::cli::resolve_project(lun, query) {
                    Ok(project) => Some(project.id),
                    Err(e) => {
                        self.close_palette();
                        self.message = Some((e.to_string(), true));
                        return true;
                    }
                }
            }
            _ => return false,
        };
        self.enter_view(View::NewTask, title);
        if let Some(project_id) = project_id {
            if let Some(FormState::NewTask(draft)) = self.form.as_mut() {
                if let Some(pos) = self.data.projects.iter().position(|p| p.id == project_id) {
                    draft.project_index = pos;
                }
            }
        }
        true
    }

    fn finish_command(
        &mut self,
        command: String,
        cli_app: &crate::cli::App,
        args: &[String],
        result: crate::db::Result<String>,
    ) {
        match result {
            Ok(text) => {
                let _ = self.refresh_from_store();
                self.select_context_from_args(cli_app, args);
                self.record_view_transition(View::Output);
                self.output = Some(CommandOutput {
                    command,
                    text,
                    is_error: false,
                });
            }
            Err(e) => {
                let _ = self.refresh_from_store();
                self.record_view_transition(View::Output);
                self.output = Some(CommandOutput {
                    command,
                    text: format!("lun: {e}"),
                    is_error: true,
                });
            }
        }
        self.output_scroll = 0;
        self.view = View::Output;
        self.close_palette();
        self.message = None;
    }

    fn submit_prompt_answer(&mut self) {
        let answer = std::mem::take(&mut self.palette_query);
        let Some(session) = self.prompt_session.as_mut() else {
            return;
        };
        session.answers.push(answer);
        if session.answers.len() < session.prompts.len() {
            self.palette_selected = 0;
            return;
        }
        let command = session.args.join(" ");
        let args = session.args.clone();
        let mut bytes = Vec::new();
        for answer in &session.answers {
            bytes.extend_from_slice(answer.as_bytes());
            bytes.push(b'\n');
        }
        self.prompt_session = None;
        let Some(root) = self.root.as_deref() else {
            self.close_palette();
            self.message = Some((
                "no store attached — command execution unavailable".to_string(),
                true,
            ));
            return;
        };
        let cli_app = match crate::cli::App::open(root) {
            Ok(app) => app,
            Err(e) => {
                self.close_palette();
                self.message = Some((format!("opening command app failed: {e}"), true));
                return;
            }
        };
        let mut reader = BufReader::new(Cursor::new(bytes));
        let result = crate::cli::run_result_in_reader(&cli_app, &args, &mut reader, Some(root));
        self.finish_command(command, &cli_app, &args, result);
    }

    /// Execute the current TUI command line.
    pub fn run_command(&mut self) {
        if self.prompt_session.is_some() {
            self.submit_prompt_answer();
            return;
        }
        let line = self.palette_query.trim().to_string();
        if line.is_empty() {
            self.close_palette();
            if self.output.is_some() {
                self.view = View::Output;
            }
            return;
        }
        self.record_command_history(&line);
        if line == "quit" {
            self.close_palette();
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
        let args = match crate::cli::split_command_line(&line) {
            Ok(args) => self.normalize_command_args(args),
            Err(e) => {
                self.output = Some(CommandOutput {
                    command: line.clone(),
                    text: format!("lun: {e}"),
                    is_error: true,
                });
                self.output_scroll = 0;
                self.view = View::Output;
                self.close_palette();
                return;
            }
        };
        self.execute_cli_args(args, line);
    }

    /// Switch views, resolving task/log subjects. `rest` is the command
    /// line text after the command name.
    pub fn enter_view(&mut self, view: View, rest: &str) {
        self.close_palette();
        if !self.can_leave_current_view() {
            return;
        }
        self.mode = Mode::Normal;
        self.pending_g = false;
        self.statusline_open = false;
        self.statusline_query.clear();
        self.message = None;
        match view {
            View::Output => {
                self.record_view_transition(View::Output);
                self.view = View::Output;
            }
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
                self.record_view_transition(View::Task);
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
                self.record_view_transition(View::Log);
                self.view = View::Log;
            }
            View::NewTask => {
                self.begin_new_task_form();
                if let Some(FormState::NewTask(d)) = self.form.as_mut() {
                    if !rest.trim().is_empty() {
                        d.title = rest.trim().to_string();
                    }
                }
                self.record_view_transition(View::NewTask);
                self.view = View::NewTask;
            }
            View::NewProject => {
                self.begin_new_project_form();
                self.record_view_transition(View::NewProject);
                self.view = View::NewProject;
            }
            View::MoveTask => match self.begin_move_task_form() {
                Ok(()) => {
                    self.record_view_transition(View::MoveTask);
                    self.view = View::MoveTask;
                }
                Err(e) => self.message = Some((e, true)),
            },
            _ => {
                self.record_view_transition(view);
                self.view = view;
            }
        }
    }

    fn can_leave_current_view(&mut self) -> bool {
        if self.notes_modified() {
            self.message = Some((
                "unsaved note edits — press Esc in insert mode first".to_string(),
                true,
            ));
            return false;
        }
        true
    }

    fn record_view_transition(&mut self, next: View) {
        if self.view != next {
            self.view_history.push(self.view);
            self.forward_view_history.clear();
        }
        self.previous_view = self.view;
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
        if self.prompt_session.is_none() {
            self.palette_query.clear();
        }
        self.palette_selected = 0;
        self.palette_vim_nav = false;
        self.stop_history_navigation();
        self.clear_leader_sequence();
    }

    pub fn close_command_prompt(&mut self) {
        self.close_palette();
        if self.output.is_some() {
            self.view = View::Output;
        }
    }

    fn close_palette(&mut self) {
        self.palette_open = false;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.palette_vim_nav = false;
        self.prompt_session = None;
        self.stop_history_navigation();
        self.clear_leader_sequence();
    }

    pub fn enter_palette_vim_nav(&mut self) {
        if self.prompt_session.is_none() {
            self.palette_vim_nav = true;
        }
    }

    pub fn exit_palette_vim_nav(&mut self) {
        self.palette_vim_nav = false;
    }

    pub fn clear_leader_sequence(&mut self) {
        self.pending_space = false;
        self.pending_space_f = false;
    }

    pub fn open_task_project_finder(&mut self) {
        self.open_palette();
        self.palette_query = "status ".to_string();
        self.palette_selected = 0;
    }

    pub fn open_log_finder(&mut self) {
        self.open_palette();
        self.palette_query = "grep ".to_string();
        self.palette_selected = 0;
    }

    /// Append a typed character to the palette query; clamp selection.
    pub fn palette_type(&mut self, ch: char) {
        self.stop_history_navigation();
        self.palette_vim_nav = false;
        self.palette_query.push(ch);
        let n = self.command_suggestions().len();
        if n == 0 {
            self.palette_selected = 0;
        } else if self.palette_selected >= n {
            self.palette_selected = n - 1;
        }
    }

    pub fn palette_backspace(&mut self) {
        self.stop_history_navigation();
        self.palette_vim_nav = false;
        self.palette_query.pop();
        if self.prompt_session.is_some() {
            return;
        }
        let n = self.command_suggestions().len();
        if n == 0 {
            self.palette_selected = 0;
        } else if self.palette_selected >= n {
            self.palette_selected = n - 1;
        }
    }

    pub fn palette_up(&mut self) {
        let n = self.command_suggestions().len();
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
        let n = self.command_suggestions().len();
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
        if self.view == View::Output {
            self.output_scroll_by(dir);
            return;
        }
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
            View::Output => self.output_scroll = 0,
            View::Project => self.project_selected = 0,
            _ => self.task_selected = 0,
        }
    }

    pub fn jump_bottom(&mut self) {
        self.pending_g = false;
        match self.view {
            View::Output => {
                self.output_scroll = self
                    .output
                    .as_ref()
                    .map(|o| o.text.lines().count().saturating_sub(1))
                    .unwrap_or(0);
            }
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
            View::Output => self.output_scroll_by(dir * self.output_page_rows.max(1) as i32),
            View::Project => self.project_nav(dir * 5, false),
            _ => self.task_nav(dir * 5),
        }
    }

    pub fn output_scroll_by(&mut self, dir: i32) {
        let current = self.output_scroll as i32;
        self.output_scroll = (current + dir).max(0) as usize;
    }

    pub fn update_layout_metrics(&mut self, total_height: u16) {
        self.output_page_rows = total_height.saturating_sub(5).max(1) as usize;
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
        if !self.can_leave_current_view() {
            return;
        }
        if self.view == View::Task && self.task_focus != TaskFocus::Summary {
            self.task_focus = TaskFocus::Summary;
            self.task_item_selected = 0;
            return;
        }
        self.go_view_back();
    }

    pub fn go_view_back(&mut self) {
        if !self.can_leave_current_view() {
            return;
        }
        if self.view != View::Initial {
            if let Some(prev) = self.view_history.pop() {
                self.forward_view_history.push(self.view);
                self.previous_view = self.view;
                self.view = prev;
            }
        }
    }

    pub fn go_view_forward(&mut self) {
        if !self.can_leave_current_view() {
            return;
        }
        if let Some(next) = self.forward_view_history.pop() {
            if self.view != next {
                self.view_history.push(self.view);
            }
            self.previous_view = self.view;
            self.view = next;
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
            lun.reopen_task(task_id, Some("doing"), None, None)
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
        self.record_view_transition(View::Task);
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
            let links = match Self::add_file_links(lun, LinkTarget::Task(task_id), &paths) {
                Ok(v) => v,
                Err(e) => {
                    self.message = Some((e, true));
                    return;
                }
            };
            for (filename, uri) in links {
                if !notes.is_empty() {
                    notes.push('\n');
                }
                notes.push_str(&format!("- [{filename}]({uri})"));
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
        if let Err(e) = Self::add_file_links(lun, LinkTarget::Project(project.id), &paths) {
            self.message = Some((e, true));
            return;
        }
        let _ = self.refresh_from_store();
        self.message = Some((
            format!(
                "Linked {} file path(s) on project {}",
                paths.len(),
                project.project_key
            ),
            false,
        ));
    }

    fn add_file_links(
        lun: &Lun,
        target: LinkTarget,
        paths: &[std::path::PathBuf],
    ) -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            let filename = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let uri = Self::file_uri_from_path(path);
            lun.add_link(target, &filename, &uri, None, None)
                .map_err(|e| format!("link failed: {e}"))?;
            out.push((filename, uri));
        }
        Ok(out)
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
                self.record_view_transition(View::Log);
                self.log_subject = Some(subject);
                self.view = View::Log;
                self.mode = Mode::Normal;
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    fn decode_file_uri(uri: &str) -> Option<std::path::PathBuf> {
        let rest = uri.strip_prefix("file://")?;
        let path = if let Some(r) = rest.strip_prefix("localhost") {
            r
        } else if rest.starts_with('/') {
            rest
        } else {
            return None;
        };
        if !path.starts_with('/') {
            return None;
        }
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
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            Some(std::path::PathBuf::from(std::ffi::OsString::from_vec(
                bytes,
            )))
        }
        #[cfg(not(unix))]
        {
            String::from_utf8(bytes).ok().map(std::path::PathBuf::from)
        }
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
        #[cfg(unix)]
        let raw: Vec<u8> = {
            use std::os::unix::ffi::OsStrExt;
            abs.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let raw: Vec<u8> = abs.to_string_lossy().into_owned().into_bytes();
        let mut encoded = String::with_capacity(raw.len() + 8);
        for b in raw {
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
                if let Some(path) = Self::decode_file_uri(&token) {
                    return Some(path);
                }
                Some(std::path::PathBuf::from(token))
            })
            .collect()
    }
}
