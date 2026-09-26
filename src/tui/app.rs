//! TUI app state: current view, palette filter/selection, transient
//! message line, and the slash command table.

use super::data::TuiData;

/// Where the app is looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Initial,
    Status,
    Board,
    Project,
    Help,
    /// /task, /new-task, /log, /config — "planned for a later phase".
    Placeholder,
}

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Initial => "Initial",
            View::Status => "Status",
            View::Board => "Board",
            View::Project => "Project",
            View::Help => "Help",
            View::Placeholder => "Coming Soon",
        }
    }
}

/// One slash command in the palette.
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    /// Which view it lands on; `None` = quit.
    pub view: Option<View>,
}

/// The full palette, in the plan's order.
pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand { name: "/status", description: "Show global status (projects + tasks)", view: Some(View::Status) },
    SlashCommand { name: "/board", description: "Show kanban board for current project", view: Some(View::Board) },
    SlashCommand { name: "/project", description: "Select or view a project", view: Some(View::Project) },
    SlashCommand { name: "/task", description: "View or edit a task (planned: Phase 6)", view: Some(View::Placeholder) },
    SlashCommand { name: "/new-task", description: "Create a new task in current project (planned: Phase 6)", view: Some(View::Placeholder) },
    SlashCommand { name: "/log", description: "Show recent logs (planned: Phase 6)", view: Some(View::Placeholder) },
    SlashCommand { name: "/config", description: "View configuration (planned)", view: Some(View::Placeholder) },
    SlashCommand { name: "/help", description: "Show help and keybindings", view: Some(View::Help) },
    SlashCommand { name: "/quit", description: "Exit lun", view: None },
];

/// The whole TUI application (state only — no IO).
#[derive(Debug, Clone)]
pub struct App {
    pub data: TuiData,
    pub view: View,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub project_selected: usize,
    /// (text, is_error) shown on the message line above the hint bar.
    pub message: Option<(String, bool)>,
    pub quit: bool,
}

impl App {
    pub fn new(data: TuiData) -> Self {
        Self {
            data,
            view: View::Initial,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            project_selected: 0,
            message: None,
            quit: false,
        }
    }

    /// Palette rows after filtering by the current query.
    ///
    /// The query is typed WITHOUT the leading `/` (the prompt shows `› /<query>`),
    /// so compare against the command name minus its slash.
    pub fn filtered_commands(&self) -> Vec<&'static SlashCommand> {
        SLASH_COMMANDS
            .iter()
            .filter(|c| c.name.trim_start_matches('/').starts_with(&self.palette_query.to_lowercase()))
            .collect()
    }

    /// Execute the selected (or given) slash command.
    pub fn run_command(&mut self, index: usize) {
        let filtered = self.filtered_commands();
        let Some(cmd) = filtered.get(index) else {
            self.message = Some((format!("no command matches '{}'", self.palette_query), true));
            self.palette_open = false;
            self.palette_query.clear();
            return;
        };
        match cmd.view {
            None => self.quit = true,
            Some(view) => self.view = view,
        }
        self.palette_open = false;
        self.palette_query.clear();
        self.palette_selected = 0;
    }

    /// Open the palette (reset state).
    pub fn open_palette(&mut self) {
        self.palette_open = true;
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
}
