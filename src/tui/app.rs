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

use super::data::TuiData;

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
    Help,
    /// /new-task, /config — "planned for a later phase".
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
    SlashCommand { name: "/status", description: "Show global status (projects + tasks)", view: Some(View::Status) },
    SlashCommand { name: "/board", description: "Show kanban board for current project", view: Some(View::Board) },
    SlashCommand { name: "/project", description: "Select or view a project", view: Some(View::Project) },
    SlashCommand { name: "/task", description: "View a task: /task <T-00N|title> (default: current task)", view: Some(View::Task) },
    SlashCommand { name: "/new-task", description: "Create a new task in current project (planned: Phase 7)", view: Some(View::Placeholder) },
    SlashCommand { name: "/log", description: "Show logs: /log <project|task> (default: current project)", view: Some(View::Log) },
    SlashCommand { name: "/config", description: "View configuration (planned)", view: Some(View::Placeholder) },
    SlashCommand { name: "/help", description: "Show help and keybindings", view: Some(View::Help) },
    SlashCommand { name: "/quit", description: "Exit lun", view: None },
];

/// The whole TUI application (state only — no IO).
#[derive(Debug, Clone)]
pub struct App {
    pub data: TuiData,
    pub view: View,
    pub mode: Mode,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub project_selected: usize,
    /// Task list selection; doubles as the "current task" for the task
    /// view and note editing.
    pub task_selected: usize,
    /// What the log view shows (set by `/log`, `:status <q>`, `/log` default).
    pub log_subject: Option<super::data::LogSubject>,
    /// Statusline: `:` opens it; the remainder of the line is the query.
    pub statusline_open: bool,
    pub statusline_query: String,
    /// Draft buffer for the current task's notes while in insert mode.
    pub notes_draft: String,
    /// Whether the draft differs from the last-committed notes text.
    pub notes_dirty: bool,
    /// (text, is_error) shown on the message line above the hint bar.
    pub message: Option<(String, bool)>,
    pub quit: bool,
}

impl App {
    pub fn new(data: TuiData) -> Self {
        Self {
            data,
            view: View::Initial,
            mode: Mode::Normal,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            project_selected: 0,
            task_selected: 0,
            log_subject: None,
            statusline_open: false,
            statusline_query: String::new(),
            notes_draft: String::new(),
            notes_dirty: false,
            message: None,
            quit: false,
        }
    }

    /// The current task (selection clamped into range).
    pub fn current_task(&self) -> Option<&crate::db::Task> {
        if self.data.tasks.is_empty() {
            return None;
        }
        self.data.tasks.get(self.task_selected.min(self.data.tasks.len() - 1))
    }

    /// Whether the current task's notes buffer has unsaved edits.
    pub fn notes_modified(&self) -> bool {
        self.mode == Mode::Insert && self.notes_dirty
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
                    self.message =
                        Some(("unsaved note edits — press Esc in insert mode first".to_string(), true));
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
            self.message =
                Some(("unsaved note edits — press Esc in insert mode first".to_string(), true));
            return;
        }
        self.mode = Mode::Normal;
        self.statusline_open = false;
        self.statusline_query.clear();
        self.message = None;
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
                self.view = View::Task;
            }
            View::Log => {
                if rest.is_empty() {
                    // Default: the current project's log.
                    self.log_subject = Some(super::data::LogSubject::Project(self.data.current_project));
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
            [] => Err(format!("no task has key or title '{q}' (try `:status` to list)")),
            [i] => {
                self.task_selected = *i;
                Ok(())
            }
            many => Err(format!(
                "ambiguous task '{q}': {} share this title: {} — use the task key",
                many.len(),
                many
                    .iter()
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
        self.palette_selected = if self.palette_selected == 0 { n - 1 } else { self.palette_selected - 1 };
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
        let n = self.data.tasks.len();
        self.task_selected =
            ((self.task_selected as i32 + dir).rem_euclid(n as i32)) as usize;
    }

    /// `t` in normal mode: jump to the current task's detail view.
    pub fn open_current_task(&mut self) {
        if self.current_task().is_none() {
            self.message = Some(("no tasks to show".to_string(), true));
            return;
        }
        if self.notes_modified() {
            self.message =
                Some(("unsaved note edits — press Esc in insert mode first".to_string(), true));
            return;
        }
        self.mode = Mode::Normal;
        self.statusline_open = false;
        self.statusline_query.clear();
        self.view = View::Task;
    }

    /// `e` in normal mode (task view): start editing the current task's
    /// notes. The buffer starts from the last-committed notes text
    /// (Phase 6 stores nothing yet — see Phase 7 for persistence).
    pub fn enter_notes_edit(&mut self) {
        if self.current_task().is_none() || self.view != View::Task {
            return;
        }
        if self.mode == Mode::Insert && self.notes_dirty {
            return; // already editing
        }
        self.mode = Mode::Insert;
        self.notes_draft.clear();
        self.notes_dirty = false;
        self.statusline_open = false;
        self.statusline_query.clear();
        self.message = Some((
            "note: editing notes — Esc to finish, Ctrl-S to save".to_string(),
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

    /// `Esc` in insert mode: back to normal mode. (Phase 6 keeps the edit
    /// as an unsaved draft; Phase 7 wires commit/persistence.)
    pub fn exit_insert(&mut self) {
        if self.mode != Mode::Insert {
            return;
        }
        self.mode = Mode::Normal;
        self.notes_dirty = false;
        if !self.notes_draft.is_empty() {
            self.message = Some((
                "note edited (draft — saving lands with Phase 7)".to_string(),
                false,
            ));
        }
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
            self.message = Some((format!("unknown quick action '{cmd}' (Phase 6: status)"), true));
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
}
