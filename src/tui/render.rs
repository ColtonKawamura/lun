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
            "/",
            Style::default().fg(t::CYAN),
        );
        let query = format!("{}_", app.palette_query);
        put(
            buf,
            area.left() + 3,
            prompt_y,
            &query,
            Style::default().fg(t::CYAN),
        );
    } else {
        // Hint text depends on mode (Phase 6): insert mode advertises the
        // note-editing keys; the statusline shows its own prompt + query.
        let hint: String = if app.statusline_open {
            format!("status {}", app.statusline_query)
        } else if app.mode == super::app::Mode::Insert {
            "inserting note — esc back to normal, ctrl-s to save".to_string()
        } else {
            " type \"/\" for commands, \":\" for quick actions, \"q\" to quit".to_string()
        };
        put(
            buf,
            area.left() + 2,
            prompt_y,
            hint.as_str(),
            Style::default().fg(
                if app.statusline_open || app.mode == super::app::Mode::Insert {
                    t::CYAN
                } else {
                    t::DIM
                },
            ),
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
        let names = ["Todo", "In Progress", "Review", "Done"];
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

fn project_counts(app: &App, project_id: i64) -> (usize, usize, usize) {
    let (mut open, mut review, mut done) = (0, 0, 0);
    for task in &app.data.tasks {
        if task.project_id != Some(project_id) {
            continue;
        }
        match task.status.as_str() {
            "todo" | "in-progress" => open += 1,
            "review" => review += 1,
            "done" => done += 1,
            _ => {}
        }
    }
    (open, review, done)
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
        "  KEY    NAME              STATUS        OPEN  REVIEW  DONE",
        Style::default().fg(t::DIM),
    );
    y += 1;
    for p in &app.data.projects {
        if y >= area.bottom() {
            return;
        }
        let (o, r, d) = project_counts(app, p.id);
        // Columns match the header: key@2, name@9 (17 wide), status@27
        // (14 wide), open@41, review@47, done@55.
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
            &format!("{:<6}", o),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 47,
            y,
            &format!("{:<8}", r),
            Style::default().fg(t::TEXT),
        );
        put(
            buf,
            area.left() + 55,
            y,
            &format!("{:<6}", d),
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
    // Four kanban columns side by side.
    let col_w = (area.width.saturating_sub(6) / 4).max(12);
    let cols = app.data.board_columns(p.id);
    let names = ["Todo", "In Progress", "Review", "Done"];
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
        let (o, r, d) = project_counts(app, p.id);
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
            &format!("{:<12}open {}  review {}  done {}", p.status, o, r, d),
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
    let rows: [(&str, &str); 20] = [
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
            "esc / backspace",
            "close the palette/statusline, or go back",
        ),
        ("q", "quit lun"),
        ("/task /log", "/task <T-00N|title>, /log <project|task>"),
        ("/new-task", "open the new-task form"),
        ("/new-project", "open the new-project form"),
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
            "up/down field · left/right choices · type text · enter on create",
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
            ["todo", "in-progress", "review", "done"][draft.status_index].to_string(),
            t::status_style(["todo", "in-progress", "review", "done"][draft.status_index]),
        ),
        (
            "Priority",
            ["low", "med", "high"][draft.priority_index].to_string(),
            Style::default().fg(t::TEXT),
        ),
        ("Assignee", draft.assignee.clone(), Style::default().fg(t::TEXT)),
        ("Branch", draft.branch.clone(), Style::default().fg(t::CYAN)),
        ("Labels", draft.labels.clone(), Style::default().fg(t::TEXT)),
        ("Create", "press Enter".to_string(), Style::default().fg(t::DONE)),
    ];
    for (idx, (label, value, style)) in rows.into_iter().enumerate() {
        if y >= area.bottom() {
            break;
        }
        form_row(buf, area.left(), y, draft.field == idx, label, &value, style);
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
            "up/down field · left/right status · type text · enter on create",
            Style::default().fg(t::DIM),
        );
        y += 2;
    }
    let rows = [
        ("Name", draft.name.clone(), Style::default().fg(t::TEXT)),
        (
            "Status",
            ["planning", "active", "in-progress", "done"][draft.status_index].to_string(),
            t::status_style(["planning", "active", "in-progress", "done"][draft.status_index]),
        ),
        ("Create", "press Enter".to_string(), Style::default().fg(t::DONE)),
    ];
    for (idx, (label, value, style)) in rows.into_iter().enumerate() {
        if y >= area.bottom() {
            break;
        }
        form_row(buf, area.left(), y, draft.field == idx, label, &value, style);
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
            "up/down field · left/right project · enter on move",
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
        ("Move", "press Enter".to_string(), Style::default().fg(t::DONE)),
    ];
    for (idx, (label, value, style)) in rows.into_iter().enumerate() {
        if y >= area.bottom() {
            break;
        }
        form_row(buf, area.left(), y, draft.field == idx, label, &value, style);
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

/// The slash-command palette overlay (bottom-anchored above the separator).
fn paint_palette(buf: &mut Buffer, area: Rect, app: &App) {
    let sep_y = area.bottom().saturating_sub(2);
    let available_rows = sep_y.saturating_sub(area.top()) as usize;
    if available_rows == 0 {
        return;
    }

    let filtered = app.filtered_commands();
    if filtered.is_empty() {
        put(
            buf,
            area.left(),
            sep_y.saturating_sub(1),
            &format!("(no command matches \"/{}\")", app.palette_query),
            Style::default().fg(t::ERROR),
        );
        return;
    }

    let total_rows = filtered.len() + 2; // heading + underline + command rows
    let render_rows = total_rows.min(available_rows);
    if render_rows == 0 {
        return;
    }

    let selected = app.palette_selected.min(filtered.len().saturating_sub(1));
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
        let cmd = filtered[i];
        let style = if i == selected {
            t::selected_style()
        } else {
            Style::default()
        };
        put(
            buf,
            area.left(),
            y,
            cmd.name,
            style.fg(if i == selected { t::BG } else { t::CYAN }),
        );
        let desc_style = Style::default().fg(if i == selected { t::BG } else { t::DIM });
        put(buf, area.left() + 16, y, cmd.description, desc_style);
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
