//! Pure rendering: paints one frame of the TUI into a `ratatui` buffer.
//! Everything here is side-effect free (no IO, no global state) so
//! tests/phase5.rs can render headlessly with
//! `ratatui::DefaultTerminal`/`Buffer::empty(Rect)`.

use crate::tui::app::{App, View};
use crate::tui::theme as t;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

/// Paint one full frame (content + hint bar + optional palette overlay).
///
/// Bottom bar layout (per docs/plan.md): the magenta separator sits on the
/// second-to-last line, the `›  type "/"...` prompt line on the last line,
/// and a transient message (if any) on the line above the separator.
pub fn paint(buf: &mut Buffer, area: Rect, app: &App) {
    FillBg.render(area, buf);
    let prompt_y = area.bottom().saturating_sub(1);
    let sep_y = prompt_y.saturating_sub(1);
    let content = Rect {
        width: area.width,
        height: sep_y.saturating_sub(area.top()),
        x: area.left(),
        y: area.top(),
    };

    match app.view {
        View::Initial => paint_initial(buf, content, app),
        View::Output => paint_output(buf, content, app),
        View::Status => paint_status(buf, content, app),
        View::Board => paint_board(buf, content, app),
        View::Project => paint_project(buf, content, app),
        View::Task => paint_task(buf, content, app),
        View::Log => paint_log(buf, content, app),
        View::NewTask => paint_new_task(buf, content, app),
        View::NewProject => paint_new_project(buf, content, app),
        View::MoveTask => paint_move_task(buf, content, app),
        View::Help => paint_help(buf, content),
        View::Placeholder => paint_placeholder(buf, content, app),
    }

    if let Some((msg, is_err)) = &app.message {
        let style = if *is_err {
            Style::default().fg(t::ERROR).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t::DONE)
        };
        put(
            buf,
            area.left(),
            sep_y.saturating_sub(1),
            msg.as_str(),
            style,
        );
    }
    put(
        buf,
        area.left(),
        sep_y,
        &"-".repeat(area.width as usize),
        Style::default().fg(t::MAGENTA),
    );
    put(
        buf,
        area.left(),
        prompt_y,
        t::PROMPT,
        Style::default().fg(t::PURPLE),
    );
    if app.palette_open {
        put(
            buf,
            area.left() + 2,
            prompt_y,
            "",
            Style::default().fg(t::CYAN),
        );
        let query = format!("{}_", app.current_command_prompt());
        put(
            buf,
            area.left() + 2,
            prompt_y,
            &query,
            Style::default().fg(t::CYAN),
        );
    } else {
        // Hint text depends on mode (Phase 6): insert mode advertises the
        // note-editing keys; the statusline shows its own prompt + query.
        let hint: String = if app.mode == super::app::Mode::Insert {
            "inserting note — esc back to normal, ctrl-s to save".to_string()
        } else {
            " type \"/\" or \":\" for commands, \"q\" to quit".to_string()
        };
        put(
            buf,
            area.left() + 2,
            prompt_y,
            hint.as_str(),
            Style::default().fg(if app.mode == super::app::Mode::Insert {
                t::CYAN
            } else {
                t::DIM
            }),
        );
    }

    if app.palette_open {
        paint_palette(buf, area, app);
    }
}

struct FillBg;

impl Widget for FillBg {
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_style(area, Style::default().bg(t::BG));
    }
}

/// Write `text` at (x, y) clamped to the buffer, one style for all of it.
pub fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    for (i, ch) in text.chars().enumerate() {
        let cx = x.saturating_add(i as u16);
        if cx >= buf.area().right() || y >= buf.area().bottom() {
            return;
        }
        buf.get_mut(cx, y).set_char(ch).set_style(style);
    }
}

/// Write text with two styles (e.g. lavender ID + plain title).
pub fn put_two(buf: &mut Buffer, x: u16, y: u16, a: &str, sa: Style, b: &str, sb: Style) {
    put(buf, x, y, a, sa);
    put(buf, x.saturating_add(a.chars().count() as u16), y, b, sb);
}

/// ALL CAPS heading + magenta underline row.
fn heading(buf: &mut Buffer, x: u16, y: u16, text: &str) -> u16 {
    put(buf, x, y, &text.to_uppercase(), t::heading_style());
    put(
        buf,
        x,
        y + 1,
        &"-".repeat(text.chars().count()),
        Style::default().fg(t::MAGENTA),
    );
    y + 2
}

/// The bright purple ASCII banner (from docs/plan.md).
pub const BANNER: &[&str] = &[
    "    _                    _",
    "   | |    _   _ _ __   _| | ___  _ __",
    "   | |   | | | | '_ \\ / _` |/ _ \\| '_ \\",
    "   | |_| | |_| | | | | (_| | (_) | | | |",
    "   \\____/ \\__,_|_| |_|\\__,_|\\___/|_| |_|\u{0}",
];

fn paint_initial(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = area.top();
    for line in BANNER {
        put(
            buf,
            area.left(),
            y,
            line,
            Style::default().fg(t::PURPLE).add_modifier(Modifier::BOLD),
        );
        y += 1;
        if y >= area.bottom() {
            return;
        }
    }
    y += 1;
    let ver = format!(
        "lun v{} — CLI-first markdown task & project tracker",
        app.data.version
    );
    put(buf, area.left(), y, &ver, Style::default().fg(t::TEXT));
    y += 1;
    put(
        buf,
        area.left(),
        y,
        &"-".repeat(63).to_string(),
        Style::default().fg(t::MAGENTA),
    );
    y += 2;

    // Context block
    let ctx: [(&str, &str, Style); 4] = [
        ("Repo:", &app.data.repo_path, Style::default().fg(t::TEXT)),
        ("Branch:", &app.data.branch, Style::default().fg(t::CYAN)),
        (
            "Project:",
            app.data
                .current()
                .map(|p| p.name.as_str())
                .unwrap_or_default(),
            Style::default().fg(t::PURPLE),
        ),
        (
            "Summary:",
            &app.data.summary(),
            Style::default().fg(t::TEXT),
        ),
    ];
    for (label, value, style) in ctx {
        put(buf, area.left(), y, label, Style::default().fg(t::DIM));
        put(buf, area.left() + 10, y, value, style);
        y += 1;
        if y >= area.bottom() {
            return;
        }
    }
    y += 1;

    // Board preview for the current project
    if let Some(p) = app.data.current() {
        let title = format!("Board ({})", p.name);
        y = heading(buf, area.left(), y, &title);
        if y >= area.bottom() {
            return;
        }
        let cols = app.data.board_columns(p.id);
        let names = ["Todo", "Doing", "Follow-Up", "Blocked", "Done"];
        for (i, tasks) in cols.iter().enumerate() {
            if y >= area.bottom() {
                return;
            }
            put(buf, area.left(), y, names[i], t::status_style(names[i]));
            y += 1;
            for task in tasks {
                if y >= area.bottom() {
                    return;
                }
                put_two(
                    buf,
                    area.left() + 4,
                    y,
                    &format!("{}  ", task.task_key),
                    Style::default().fg(t::LAVENDER),
                    &task.title,
                    Style::default().fg(t::TEXT),
                );
                y += 1;
            }
        }
    }
}

fn paint_output(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = heading(buf, area.left(), area.top(), "Command Output");
    if y >= area.bottom() {
        return;
    }
    let Some(output) = app.output.as_ref() else {
        put(
            buf,
            area.left(),
            y,
            "Run a command with / or : to display output here.",
            Style::default().fg(t::DIM),
        );
        return;
    };
    put(
        buf,
        area.left(),
        y,
        &format!("$ {}", output.command),
        Style::default().fg(t::CYAN),
    );
    y += 2;
    let max_rows = area.bottom().saturating_sub(y) as usize;
    let total_lines = output.text.lines().count();
    let start = app.output_scroll.min(total_lines.saturating_sub(max_rows));
    let mut lines = output.text.lines().skip(start).peekable();
    while let Some(line) = lines.next() {
        if y >= area.bottom() {
            break;
        }
        if !output.is_error
            && line.starts_with("Summary: ")
            && y + 1 < area.bottom()
            && !line.is_empty()
        {
            put(
                buf,
                area.left(),
                y,
                &"-".repeat(area.width as usize),
                Style::default().fg(t::MAGENTA),
            );
            y += 1;
            if y >= area.bottom() {
                break;
            }
        }
        paint_output_line(buf, area.left(), y, line, output.is_error);
        y += 1;
        if line.is_empty() && lines.peek().is_none() {
            break;
        }
    }
}

fn paint_output_line(buf: &mut Buffer, x: u16, y: u16, line: &str, is_error: bool) {
    if is_error {
        put(
            buf,
            x,
            y,
            line,
            Style::default().fg(t::ERROR).add_modifier(Modifier::BOLD),
        );
        return;
    }
    if let Some(title) = emphasized_heading(line) {
        put(buf, x, y, &title, t::heading_style());
        return;
    }
    if is_rule_line(line) {
        put(buf, x, y, line, Style::default().fg(t::MAGENTA));
        return;
    }
    if let Some(section) = section_heading(line) {
        put(
            buf,
            x,
            y,
            &section,
            Style::default().fg(t::PURPLE).add_modifier(Modifier::BOLD),
        );
        return;
    }
    if line.starts_with("Summary: ") {
        put(
            buf,
            x,
            y,
            line,
            Style::default().fg(t::TEXT).add_modifier(Modifier::BOLD),
        );
        return;
    }
    if let Some((label, spacing, value)) = detail_label_value(line) {
        put(
            buf,
            x,
            y,
            label,
            Style::default().fg(t::DIM).add_modifier(Modifier::BOLD),
        );
        put(
            buf,
            x + label.chars().count() as u16,
            y,
            spacing,
            Style::default().fg(t::DIM),
        );
        put(
            buf,
            x + (label.chars().count() + spacing.chars().count()) as u16,
            y,
            value,
            detail_value_style(label, value),
        );
        return;
    }
    if let Some((prefix, status, rest)) = status_count_line(line) {
        put(buf, x, y, prefix, Style::default().fg(t::DIM));
        put(
            buf,
            x + prefix.chars().count() as u16,
            y,
            status,
            t::status_style(status.trim_end_matches(':')).add_modifier(Modifier::BOLD),
        );
        put(
            buf,
            x + (prefix.chars().count() + status.chars().count()) as u16,
            y,
            rest,
            Style::default().fg(t::TEXT),
        );
        return;
    }
    if table_header_line(line) {
        put(
            buf,
            x,
            y,
            line,
            Style::default().fg(t::DIM).add_modifier(Modifier::BOLD),
        );
        return;
    }
    put(buf, x, y, line, Style::default().fg(t::TEXT));
}

fn emphasized_heading(line: &str) -> Option<String> {
    if let Some(body) = line.strip_prefix("**").and_then(|s| s.strip_suffix("**")) {
        return Some(body.to_string());
    }
    if line.starts_with("Project: ") || line.starts_with("Task ") || line.starts_with("Board: ") {
        return Some(line.to_string());
    }
    None
}

fn is_rule_line(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| matches!(c, '=' | '-'))
}

fn section_heading(line: &str) -> Option<String> {
    match line.trim() {
        "Overview" | "Tasks" | "Checklist" | "Notes:" | "Attachments:" | "Links:" | "History"
        | "History (log):" | "Last Commit:" | "Tasks by Status:" => {
            Some(line.trim_end_matches(':').to_string())
        }
        _ => None,
    }
}

fn detail_label_value(line: &str) -> Option<(&str, &str, &str)> {
    if line.starts_with("- ") || line.starts_with("    ") {
        return None;
    }
    let colon = line.find(':')?;
    let label = &line[..=colon];
    if !label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | ' ' | '-' | '*'))
    {
        return None;
    }
    let rest = &line[colon + 1..];
    let pad_len = rest.chars().take_while(|c| *c == ' ').count();
    let (spacing, value) = rest.split_at(pad_len);
    Some((label, spacing, value))
}

fn detail_value_style(label: &str, value: &str) -> Style {
    match label.trim_end_matches(':') {
        "ID" => Style::default().fg(t::LAVENDER),
        "Status" => t::status_style(value.trim()),
        "Branch" => Style::default().fg(t::CYAN),
        _ => Style::default().fg(t::TEXT),
    }
}

fn status_count_line(line: &str) -> Option<(&str, &str, &str)> {
    let body = line.strip_prefix("- ")?;
    let colon = body.find(':')?;
    let status = &body[..=colon];
    if !matches!(
        status.trim_end_matches(':'),
        "todo" | "doing" | "follow-up" | "blocked" | "done"
    ) {
        return None;
    }
    Some(("- ", status, &body[colon + 1..]))
}

fn table_header_line(line: &str) -> bool {
    line.starts_with("ID")
        && (line.contains("Project") || line.contains("Title"))
        && line.contains("Status")
}

fn project_counts(app: &App, project_id: i64) -> [usize; 5] {
    let mut counts = [0; 5];
    for task in &app.data.tasks {
        if task.project_id != Some(project_id) {
            continue;
        }
        match task.status.as_str() {
            "todo" => counts[0] += 1,
            "doing" => counts[1] += 1,
            "follow-up" => counts[2] += 1,
            "blocked" => counts[3] += 1,
            "done" => counts[4] += 1,
            _ => {}
        }
    }
    counts
}

fn paint_status(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = heading(buf, area.left(), area.top(), "Status");
    if y >= area.bottom() {
        return;
    }
    put(
        buf,
        area.left(),
        y,
        "PROJECTS",
        Style::default().fg(t::PURPLE),
    );
    y += 1;
    if y >= area.bottom() {
        return;
    }
    put(
        buf,
        area.left(),
        y,
        "  KEY    NAME              STATUS        TODO  DOING  FOLLOW-UP  BLOCKED  DONE",
        Style::default().fg(t::DIM),
    );
    y += 1;
    for p in &app.data.projects {
        if y >= area.bottom() {
            return;
        }
        let counts = project_counts(app, p.id);
        // Columns match the header: key@2, name@9, status@27, counts after.
        put(
            buf,
            area.left() + 2,
            y,
            &p.project_key,
            Style::default().fg(t::LAVENDER),
        );
        let name: String = p.name.chars().take(17).collect();
        put(
            buf,
            area.left() + 9,
            y,
            &format!("{:<17}", name),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 27,
            y,
            &format!("{:<14}", p.status),
            t::status_style(&p.status),
        );
        put(
            buf,
            area.left() + 41,
            y,
            &format!("{:<6}", counts[0]),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 47,
            y,
            &format!("{:<7}", counts[1]),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 54,
            y,
            &format!("{:<11}", counts[2]),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 66,
            y,
            &format!("{:<9}", counts[3]),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 76,
            y,
            &format!("{:<6}", counts[4]),
            Style::default().fg(t::TEXT),
        );
        y += 1;
    }
    y += 1;
    if y >= area.bottom() {
        return;
    }
    put(
        buf,
        area.left(),
        y,
        "ALL TASKS",
        Style::default().fg(t::PURPLE),
    );
    y += 1;
    for task in &app.data.tasks {
        if y >= area.bottom() {
            return;
        }
        let proj = project_name(app, task);
        put(
            buf,
            area.left(),
            y,
            &format!("  {}   {}  ", task.task_key, proj),
            Style::default().fg(t::LAVENDER),
        );
        let title_w = 34usize.min(task.title.chars().count());
        let title: String = task.title.chars().take(title_w).collect();
        put(
            buf,
            area.left() + 24,
            y,
            &format!("{:<34}", title),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 58,
            y,
            &task.status,
            t::status_style(&task.status),
        );
        y += 1;
    }
    y += 1;
    if y < area.bottom() {
        put(
            buf,
            area.left(),
            y,
            &app.data.summary(),
            Style::default().fg(t::TEXT),
        );
    }
}

fn project_name(app: &App, task: &crate::db::Task) -> String {
    match task.project_id {
        Some(pid) => app
            .data
            .projects
            .iter()
            .find(|p| p.id == pid)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        None => "Unassigned".to_string(),
    }
}

fn paint_board(buf: &mut Buffer, area: Rect, app: &App) {
    let p = match app.data.current() {
        Some(p) => p,
        None => return,
    };
    let title = format!("Board ({})", p.name);
    let y = heading(buf, area.left(), area.top(), &title);
    if y + 1 >= area.bottom() {
        return;
    }
    // Five kanban columns side by side.
    let col_w = (area.width.saturating_sub(8) / 5).max(10);
    let cols = app.data.board_columns(p.id);
    let names = ["Todo", "Doing", "Follow-Up", "Blocked", "Done"];
    for (i, tasks) in cols.iter().enumerate() {
        let x = area.left() + (i as u16) * (col_w + 2);
        put(buf, x, y, names[i], t::status_style(names[i]));
        put(
            buf,
            x,
            y + 1,
            &"-".repeat(names[i].chars().count().min(col_w as usize))
                .to_string(),
            Style::default().fg(t::MAGENTA),
        );
        let mut ty = y + 2;
        for task in tasks {
            if ty >= area.bottom() {
                break;
            }
            put(buf, x, ty, &task.task_key, Style::default().fg(t::LAVENDER));
            let t: String = task.title.chars().take(col_w as usize - 8).collect();
            put(buf, x + 8, ty, &t, Style::default().fg(t::TEXT));
            ty += 1;
        }
    }
}

fn paint_project(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = heading(buf, area.left(), area.top(), "Project");
    if y >= area.bottom() {
        return;
    }
    for (i, p) in app.data.projects.iter().enumerate() {
        if y >= area.bottom() {
            return;
        }
        let counts = project_counts(app, p.id);
        let marker = if i == app.project_selected {
            "> "
        } else if i == app.data.current_project {
            "* "
        } else {
            "  "
        };
        put(
            buf,
            area.left(),
            y,
            &format!("{}{}  ", marker, p.project_key),
            if i == app.project_selected {
                Style::default().fg(t::PURPLE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t::LAVENDER)
            },
        );
        put(
            buf,
            area.left() + 10,
            y,
            &format!("{}   ", p.name),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 30,
            y,
            &format!(
                "{:<9}todo {}  doing {}  follow-up {}  blocked {}  done {}",
                p.status, counts[0], counts[1], counts[2], counts[3], counts[4]
            ),
            t::status_style(&p.status),
        );
        y += 1;
    }
    y += 1;
    if y < area.bottom() {
        put(
            buf,
            area.left(),
            y,
            "j/k select · enter set current project · / commands · q quit",
            Style::default().fg(t::DIM),
        );
    }
}

fn paint_help(buf: &mut Buffer, area: Rect) {
    let mut y = heading(buf, area.left(), area.top(), "Help");
    if y >= area.bottom() {
        return;
    }
    let rows: [(&str, &str); 24] = [
        ("/", "open the command palette"),
        ("?", "open the help view"),
        (":", "quick action line — :status <project|task>"),
        ("j / k / ↑ / ↓", "navigate lists and focused task details"),
        (
            "h / l / ← / →",
            "move task-detail focus between summary/notes/attachments/links",
        ),
        ("gg / G", "jump to the first / last item"),
        ("PgUp / PgDn", "jump by larger steps"),
        ("Home / End", "jump to first / last item"),
        (
            "enter",
            "select/open (project view sets current project; task opens focused item)",
        ),
        ("t", "open the current task's detail view"),
        (
            "o",
            "open the focused task note link / attachment / explicit link",
        ),
        ("c", "toggle the current task complete/reopen"),
        (
            "i / e",
            "edit the current task's notes (task view; Esc back, Ctrl-S save)",
        ),
        (
            "↑ / ↓",
            "command prompt: browse command history for this session",
        ),
        (
            "esc",
            "palette: vim prompt-nav mode / close; elsewhere: go back",
        ),
        ("backspace", "palette line edit, or go back"),
        ("<space> f f", "open finder prompt (`status `)"),
        ("<space> f g", "open log finder prompt (`log `)"),
        ("q", "quit lun"),
        ("/task /log", "/task <T-00N|title>, /log <project|task>"),
        ("/new-task", "open the new-task form"),
        ("/new proj", "open the new-project form"),
        ("/move", "move the current task to a different project"),
        ("…", "/config remains a placeholder"),
    ];
    for (key, desc) in rows {
        if y >= area.bottom() {
            return;
        }
        put(buf, area.left() + 2, y, key, Style::default().fg(t::CYAN));
        put(buf, area.left() + 16, y, desc, Style::default().fg(t::DIM));
        y += 1;
    }
}

fn form_row(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    selected: bool,
    label: &str,
    value: &str,
    value_style: Style,
) {
    let marker = if selected { "> " } else { "  " };
    put(
        buf,
        x,
        y,
        &format!("{marker}{label:<10}"),
        if selected {
            t::selected_style().fg(t::BG)
        } else {
            Style::default().fg(t::DIM)
        },
    );
    put(
        buf,
        x + 13,
        y,
        value,
        if selected {
            t::selected_style().fg(t::BG)
        } else {
            value_style
        },
    );
}

fn paint_new_task(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = heading(buf, area.left(), area.top(), "New Task");
    let Some(super::app::FormState::NewTask(draft)) = app.form() else {
        return;
    };
    if y < area.bottom() {
        put(
            buf,
            area.left(),
            y,
            "up/down field · left/right choices · type text · esc vim j/k · esc esc cancel · enter create",
            Style::default().fg(t::DIM),
        );
        y += 2;
    }
    let project = app
        .data
        .projects
        .get(draft.project_index)
        .map(|p| format!("{} [{}]", p.name, p.project_key))
        .unwrap_or_else(|| "Unassigned [P-000]".to_string());
    let rows = [
        ("Title", draft.title.clone(), Style::default().fg(t::TEXT)),
        ("Project", project, Style::default().fg(t::PURPLE)),
        (
            "Status",
            ["todo", "doing", "follow-up", "blocked", "done"][draft.status_index].to_string(),
            t::status_style(["todo", "doing", "follow-up", "blocked", "done"][draft.status_index]),
        ),
        (
            "Priority",
            ["low", "med", "high"][draft.priority_index].to_string(),
            Style::default().fg(t::TEXT),
        ),
        (
            "Assignee",
            draft.assignee.clone(),
            Style::default().fg(t::TEXT),
        ),
        ("Branch", draft.branch.clone(), Style::default().fg(t::CYAN)),
        ("Labels", draft.labels.clone(), Style::default().fg(t::TEXT)),
        (
            "Create",
            "press Enter".to_string(),
            Style::default().fg(t::DONE),
        ),
    ];
    for (idx, (label, value, style)) in rows.into_iter().enumerate() {
        if y >= area.bottom() {
            break;
        }
        form_row(
            buf,
            area.left(),
            y,
            draft.field == idx,
            label,
            &value,
            style,
        );
        y += 1;
    }
}

fn paint_new_project(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = heading(buf, area.left(), area.top(), "New Project");
    let Some(super::app::FormState::NewProject(draft)) = app.form() else {
        return;
    };
    if y < area.bottom() {
        put(
            buf,
            area.left(),
            y,
            "up/down field · left/right status · type text · esc vim j/k · esc esc cancel · enter create",
            Style::default().fg(t::DIM),
        );
        y += 2;
    }
    let rows = [
        ("Name", draft.name.clone(), Style::default().fg(t::TEXT)),
        (
            "Status",
            ["active", "inactive"][draft.status_index].to_string(),
            t::status_style(["active", "inactive"][draft.status_index]),
        ),
        (
            "Create",
            "press Enter".to_string(),
            Style::default().fg(t::DONE),
        ),
    ];
    for (idx, (label, value, style)) in rows.into_iter().enumerate() {
        if y >= area.bottom() {
            break;
        }
        form_row(
            buf,
            area.left(),
            y,
            draft.field == idx,
            label,
            &value,
            style,
        );
        y += 1;
    }
}

fn paint_move_task(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = heading(buf, area.left(), area.top(), "Move Task");
    let Some(super::app::FormState::MoveTask(draft)) = app.form() else {
        return;
    };
    if y < area.bottom() {
        put(
            buf,
            area.left(),
            y,
            "up/down field · left/right project · esc vim j/k · esc esc cancel · enter move",
            Style::default().fg(t::DIM),
        );
        y += 2;
    }
    if let Some(task) = app.current_task() {
        put(
            buf,
            area.left(),
            y,
            &format!("Task: {} {}", task.task_key, task.title),
            Style::default().fg(t::TEXT),
        );
        y += 2;
    }
    let project = app
        .data
        .projects
        .get(draft.project_index)
        .map(|p| format!("{} [{}]", p.name, p.project_key))
        .unwrap_or_else(|| "Unassigned [P-000]".to_string());
    let rows = [
        ("Project", project, Style::default().fg(t::PURPLE)),
        (
            "Move",
            "press Enter".to_string(),
            Style::default().fg(t::DONE),
        ),
    ];
    for (idx, (label, value, style)) in rows.into_iter().enumerate() {
        if y >= area.bottom() {
            break;
        }
        form_row(
            buf,
            area.left(),
            y,
            draft.field == idx,
            label,
            &value,
            style,
        );
        y += 1;
    }
}

fn paint_placeholder(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = heading(buf, area.left(), area.top(), "Config (Coming Soon)");
    if y >= area.bottom() {
        return;
    }
    put(
        buf,
        area.left(),
        y,
        "This view is planned for a later phase of lun.",
        Style::default().fg(t::DIM),
    );
    y += 1;
    if y < area.bottom() {
        put(
            buf,
            area.left(),
            y,
            &format!(
                "current project: {} [{}]",
                app.data.current().map(|p| p.name.as_str()).unwrap_or("?"),
                app.data
                    .current()
                    .map(|p| p.project_key.as_str())
                    .unwrap_or("?")
            ),
            Style::default().fg(t::TEXT),
        );
    }
}

/// The command-completion overlay (bottom-anchored above the separator).
fn paint_palette(buf: &mut Buffer, area: Rect, app: &App) {
    let sep_y = area.bottom().saturating_sub(2);
    let available_rows = sep_y.saturating_sub(area.top()) as usize;
    if available_rows == 0 {
        return;
    }

    if app.prompt_session.is_some() {
        return;
    }
    let suggestions = app.command_suggestions();
    if suggestions.is_empty() {
        put(
            buf,
            area.left(),
            sep_y.saturating_sub(1),
            &format!("(no completion matches \"{}\")", app.palette_query),
            Style::default().fg(t::ERROR),
        );
        return;
    }

    let total_rows = suggestions.len() + 2; // heading + underline + rows
    let render_rows = total_rows.min(available_rows);
    if render_rows == 0 {
        return;
    }

    let selected = app
        .palette_selected
        .min(suggestions.len().saturating_sub(1));
    let selected_row = selected + 2;
    let mut start = total_rows.saturating_sub(render_rows); // bottom-anchored by default
    if selected_row < start {
        start = selected_row;
    } else if selected_row >= start + render_rows {
        start = selected_row + 1 - render_rows;
    }

    let mut y = sep_y.saturating_sub(render_rows as u16);
    for row in start..(start + render_rows) {
        if row == 0 {
            put(buf, area.left(), y, "COMMANDS", t::heading_style());
            y += 1;
            continue;
        }
        if row == 1 {
            put(
                buf,
                area.left(),
                y,
                "--------",
                Style::default().fg(t::MAGENTA),
            );
            y += 1;
            continue;
        }
        let i = row - 2;
        let suggestion = &suggestions[i];
        let style = if i == selected {
            t::selected_style()
        } else {
            Style::default()
        };
        put(
            buf,
            area.left(),
            y,
            suggestion,
            style.fg(if i == selected { t::BG } else { t::CYAN }),
        );
        y += 1;
    }
}

// ---------------------------------------------------------------------------
// Phase 6: task view & log view
// ---------------------------------------------------------------------------

/// One label/value row of the task view: dim label at x, styled value after.
fn task_field(buf: &mut Buffer, x: u16, y: u16, label: &str, value: &str, value_style: Style) {
    put(buf, x, y, label, Style::default().fg(t::DIM));
    put(
        buf,
        x.saturating_add(label.chars().count() as u16),
        y,
        value,
        value_style,
    );
}

fn task_section(buf: &mut Buffer, x: u16, y: u16, text: &str, focused: bool) -> u16 {
    let style = if focused {
        t::selected_style().fg(t::BG)
    } else {
        Style::default().fg(t::CYAN).add_modifier(Modifier::BOLD)
    };
    put(buf, x, y, text, style);
    y + 1
}

/// Task detail view (docs/plan.md Phase 6 "Task View and Logs"): fields,
/// checklist, notes (with the insert-mode draft), attachments, links, and
/// history. History lines are the CLI's exact formatting
/// (`cli::task_view_entry_lines`), rendered here with per-line styles.
fn paint_task(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(task) = app.current_task() else {
        put(
            buf,
            area.left(),
            area.top(),
            "no task selected",
            Style::default().fg(t::DIM),
        );
        return;
    };
    let x = area.left();
    let mut y = area.top();
    let bottom = area.bottom();

    let header = format!("Task {}", task.task_key);
    put(
        buf,
        x,
        y,
        &header,
        Style::default().fg(t::PURPLE).add_modifier(Modifier::BOLD),
    );
    y += 1;
    put(
        buf,
        x,
        y,
        &"=".repeat(header.chars().count()),
        Style::default().fg(t::MAGENTA),
    );
    y += 2;
    if y >= bottom {
        return;
    }

    // Project name via snapshot lookup (DB-free).
    let project_name = task
        .project_id
        .and_then(|pid| app.data.projects.iter().find(|p| p.id == pid))
        .map(|p| p.name.as_str())
        .unwrap_or("Unassigned");
    task_field(
        buf,
        x,
        y,
        "Project:   ",
        project_name,
        Style::default().fg(t::TEXT),
    );
    y += 1;
    if y >= bottom {
        return;
    }
    task_field(
        buf,
        x,
        y,
        "Title:     ",
        &task.title,
        Style::default().fg(t::TEXT),
    );
    y += 1;
    if y >= bottom {
        return;
    }
    task_field(
        buf,
        x,
        y,
        "Status:    ",
        &task.status,
        t::status_style(&task.status),
    );
    y += 1;
    if y >= bottom {
        return;
    }
    task_field(
        buf,
        x,
        y,
        "Priority:  ",
        &task.priority,
        Style::default().fg(t::TEXT),
    );
    y += 1;
    if y >= bottom {
        return;
    }
    task_field(
        buf,
        x,
        y,
        "Assignee:  ",
        task.assignee.as_deref().unwrap_or_default(),
        Style::default().fg(t::TEXT),
    );
    y += 1;
    if y >= bottom {
        return;
    }
    task_field(
        buf,
        x,
        y,
        "Branch:    ",
        task.branch.as_deref().unwrap_or_default(),
        Style::default().fg(t::CYAN),
    );
    y += 1;
    if y >= bottom {
        return;
    }
    task_field(
        buf,
        x,
        y,
        "Created:   ",
        &crate::cli::display_ts(&task.created_at),
        Style::default().fg(t::DIM),
    );
    y += 2;
    if y >= bottom {
        return;
    }

    y = task_section(
        buf,
        x,
        y,
        "Checklist:",
        app.task_focus == super::app::TaskFocus::Summary,
    );
    if y >= bottom {
        return;
    }
    put(
        buf,
        x,
        y,
        "- [ ] (checklist editing arrives in a later phase)",
        Style::default().fg(t::DIM),
    );
    y += 2;
    if y >= bottom {
        return;
    }

    y = task_section(
        buf,
        x,
        y,
        "Notes:",
        app.task_focus == super::app::TaskFocus::Notes,
    );
    if y >= bottom {
        return;
    }
    let editing = app.mode == super::app::Mode::Insert;
    if editing {
        // Insert mode: the live draft (starts from the stored notes).
        let notes = app.notes_draft.lines().chain(std::iter::once(""));
        for line in notes {
            if y >= bottom {
                return;
            }
            put(buf, x, y, "- ", Style::default().fg(t::DIM));
            put(buf, x + 2, y, line, Style::default().fg(t::TEXT));
            y += 1;
        }
    } else if !task.notes.is_empty() {
        // Normal mode: the persisted notes (Phase 7: `e`/`i` to edit,
        // Esc/Ctrl-S to save, drop a file to insert a link).
        let note_links = app.current_note_links();
        let selected_uri = note_links.get(app.task_item_selected);
        let mut seen_links = 0usize;
        for line in task.notes.lines() {
            if y >= bottom {
                return;
            }
            let links_on_line = note_links
                .iter()
                .skip(seen_links)
                .take_while(|uri| line.contains(uri.as_str()))
                .count();
            let line_selected_index = if app.task_focus == super::app::TaskFocus::Notes
                && selected_uri.is_some()
                && selected_uri.map(|uri| line.contains(uri)).unwrap_or(false)
            {
                Some(app.task_item_selected)
            } else {
                None
            };
            let selected = app.task_focus == super::app::TaskFocus::Notes
                && selected_uri.is_some()
                && selected_uri.map(|uri| line.contains(uri)).unwrap_or(false);
            put(
                buf,
                x,
                y,
                &if let Some(idx) = line_selected_index {
                    format!(">{} ", idx + 1)
                } else {
                    "- ".to_string()
                },
                if selected {
                    t::selected_style().fg(t::BG)
                } else {
                    Style::default().fg(t::DIM)
                },
            );
            put(
                buf,
                x + 2,
                y,
                line,
                if selected {
                    t::selected_style().fg(t::BG)
                } else {
                    Style::default().fg(t::TEXT)
                },
            );
            y += 1;
            seen_links += links_on_line;
        }
    } else {
        put(
            buf,
            x,
            y,
            "- (add notes with 'e' in the task view)",
            Style::default().fg(t::DIM),
        );
        y += 1;
    }
    y += 1;
    if y >= bottom {
        return;
    }

    y = task_section(
        buf,
        x,
        y,
        "Attachments:",
        app.task_focus == super::app::TaskFocus::Attachments,
    );
    if y >= bottom {
        return;
    }
    let attachments = app.data.attachments_for_task(task.id);
    if attachments.is_empty() {
        put(
            buf,
            x,
            y,
            "- (drag a file onto the TUI to attach one)",
            Style::default().fg(t::DIM),
        );
    } else {
        for (idx, a) in attachments.into_iter().enumerate() {
            if y >= bottom {
                return;
            }
            let selected = app.task_focus == super::app::TaskFocus::Attachments
                && idx == app.task_item_selected;
            put(
                buf,
                x,
                y,
                &format!(
                    "{} {} ({})",
                    if selected { ">" } else { "-" },
                    a.filename,
                    a.stored_path
                ),
                if selected {
                    t::selected_style().fg(t::BG)
                } else {
                    Style::default().fg(t::TEXT)
                },
            );
            y += 1;
        }
    }
    y += 1;
    if y >= bottom {
        return;
    }

    y = task_section(
        buf,
        x,
        y,
        "Links:",
        app.task_focus == super::app::TaskFocus::Links,
    );
    if y >= bottom {
        return;
    }
    let links = app.data.links_for_task(task.id);
    if links.is_empty() {
        put(buf, x, y, "- (none)", Style::default().fg(t::DIM));
    } else {
        for (idx, l) in links.into_iter().enumerate() {
            if y >= bottom {
                return;
            }
            // Markdown-style link, rendered label cyan / uri dim.
            let selected =
                app.task_focus == super::app::TaskFocus::Links && idx == app.task_item_selected;
            put(
                buf,
                x,
                y,
                &format!("{} [{}] ", if selected { ">" } else { "-" }, l.label),
                if selected {
                    t::selected_style().fg(t::BG)
                } else {
                    Style::default().fg(t::CYAN)
                },
            );
            let lx = x + 2 + (l.label.chars().count() as u16) + 1;
            put(
                buf,
                lx,
                y,
                &l.uri,
                if selected {
                    t::selected_style().fg(t::BG)
                } else {
                    Style::default().fg(t::DIM)
                },
            );
            y += 1;
        }
    }
    y += 1;
    if y >= bottom {
        return;
    }

    y = task_section(buf, x, y, "History:", false);
    if y >= bottom {
        return;
    }
    for entry in app.data.logs_for_task(task.id) {
        for line in crate::cli::task_view_entry_lines(entry) {
            if y >= bottom {
                return;
            }
            // First line of an entry: timestamp (dim) + user (lavender) +
            // ACTION (bold purple); the remainder renders as detail text.
            let style = if line.starts_with('-') {
                Style::default().fg(t::DIM)
            } else {
                Style::default().fg(t::DIM)
            };
            put(buf, x, y, &line, style);
            y += 1;
        }
    }
}

/// Log view (docs/plan.md Phase 6): `Log: Task T-00N "title"` or
/// `Log: <project>` with the CLI's exact per-entity lines.
fn paint_log(buf: &mut Buffer, area: Rect, app: &App) {
    let x = area.left();
    let bottom = area.bottom();
    let mut y = area.top();

    let (header, entries, is_project) = match &app.log_subject {
        None => {
            put(
                buf,
                x,
                y,
                "no log subject — use /log <project|task>",
                Style::default().fg(t::DIM),
            );
            return;
        }
        Some(super::data::LogSubject::Task(i)) => {
            let t = match app.data.tasks.get(*i) {
                Some(t) => t,
                None => {
                    put(buf, x, y, "task vanished", Style::default().fg(t::ERROR));
                    return;
                }
            };
            (
                format!("Log: Task {} \"{}\"", t.task_key, t.title),
                app.data.logs_for_task(t.id),
                false,
            )
        }
        Some(super::data::LogSubject::Project(i)) => {
            let p = match app.data.projects.get(*i) {
                Some(p) => p,
                None => {
                    put(buf, x, y, "project vanished", Style::default().fg(t::ERROR));
                    return;
                }
            };
            (
                format!("Log: {}", p.name),
                app.data.project_log_entries(p.id),
                true,
            )
        }
    };

    put(
        buf,
        x,
        y,
        &header,
        Style::default().fg(t::PURPLE).add_modifier(Modifier::BOLD),
    );
    y += 1;
    if y >= bottom {
        return;
    }
    put(
        buf,
        x,
        y,
        &"=".repeat(header.chars().count()),
        Style::default().fg(t::MAGENTA),
    );
    y += 1;

    if entries.is_empty() {
        put(buf, x, y, "(no log entries)", Style::default().fg(t::DIM));
        return;
    }
    for entry in entries {
        let lines = if is_project {
            crate::cli::project_log_entry_lines(entry)
        } else {
            crate::cli::task_log_entry_lines(entry)
        };
        for (n, line) in lines.iter().enumerate() {
            if y + 1 >= bottom {
                return;
            }
            if n == 0 {
                // Header line: leading timestamp dim, rest plain.
                put(buf, x, y, line, Style::default().fg(t::DIM));
            } else if line.trim_start().starts_with("Commit:") {
                put(buf, x, y, line, Style::default().fg(t::TEXT));
            } else if line.trim_start().starts_with("Note:") {
                put(buf, x, y, line, Style::default().fg(t::CYAN));
            } else {
                put(buf, x, y, line, Style::default().fg(t::DIM));
            }
            y += 1;
        }
        if y + 1 >= bottom {
            return;
        }
    }
}
