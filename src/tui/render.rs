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
        View::Help => paint_help(buf, content),
        View::Placeholder => paint_placeholder(buf, content, app),
    }

    if let Some((msg, is_err)) = &app.message {
        let style = if *is_err {
            Style::default().fg(t::ERROR).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t::DONE)
        };
        put(buf, area.left(), sep_y.saturating_sub(1), msg.as_str(), style);
    }
    put(
        buf,
        area.left(),
        sep_y,
        &"-".repeat(area.width as usize),
        Style::default().fg(t::MAGENTA),
    );
    put(buf, area.left(), prompt_y, t::PROMPT, Style::default().fg(t::PURPLE));
    put(
        buf,
        area.left() + 2,
        prompt_y,
        " type \"/\" for commands, \":\" for quick actions, \"q\" to quit",
        Style::default().fg(t::DIM),
    );

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
    put(buf, x, y + 1, &"-".repeat(text.chars().count()), Style::default().fg(t::MAGENTA));
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
        put(buf, area.left(), y, line, Style::default().fg(t::PURPLE).add_modifier(Modifier::BOLD));
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
        ("Summary:", &app.data.summary(), Style::default().fg(t::TEXT)),
    ];
    for (label, value, style) in ctx {
        put(buf, area.left(), y, label, Style::default().fg(t::DIM));
        put(
            buf,
            area.left() + 10,
            y,
            value,
            style,
        );
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
    put(buf, area.left(), y, "PROJECTS", Style::default().fg(t::PURPLE));
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
    put(buf, area.left(), y, "ALL TASKS", Style::default().fg(t::PURPLE));
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
        put(buf, area.left() + 24, y, &format!("{:<34}", title), Style::default().fg(t::TEXT));
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
        put(buf, area.left(), y, &app.data.summary(), Style::default().fg(t::TEXT));
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
            &"-"
                .repeat(names[i].chars().count().min(col_w as usize))
                .to_string(),
            Style::default().fg(t::MAGENTA),
        );
        let mut ty = y + 2;
        for task in tasks {
            if ty >= area.bottom() {
                break;
            }
            put(
                buf,
                x,
                ty,
                &task.task_key,
                Style::default().fg(t::LAVENDER),
            );
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
    let rows: [(&str, &str); 9] = [
        ("/", "open the command palette"),
        ("j / k", "navigate lists (project view)"),
        ("enter", "select (project view: set current project)"),
        ("esc", "close the palette"),
        ("q", "quit lun"),
        ("/status", "global status (projects + all tasks)"),
        ("/board", "kanban board for the current project"),
        ("/project", "select or view a project"),
        ("…", "/task, /new-task, /log, /config arrive in later phases"),
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

fn paint_placeholder(buf: &mut Buffer, area: Rect, app: &App) {
    let mut y = heading(buf, area.left(), area.top(), "Coming Soon");
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
                app.data.current().map(|p| p.project_key.as_str()).unwrap_or("?")
            ),
            Style::default().fg(t::TEXT),
        );
    }
}

/// The slash-command palette overlay (replaces the content area).
fn paint_palette(buf: &mut Buffer, area: Rect, app: &App) {
    // Prompt line: "› /<query>"
    put(buf, area.left(), area.top(), t::PROMPT, Style::default().fg(t::PURPLE));
    put(buf, area.left() + 2, area.top(), "/", Style::default().fg(t::CYAN));
    put(
        buf,
        area.left() + 3,
        area.top(),
        &app.palette_query,
        Style::default().fg(t::CYAN),
    );

    let mut y = area.top() + 2;
    y = heading(buf, area.left(), y, "Commands");
    if y >= area.bottom() {
        return;
    }
    let filtered = app.filtered_commands();
    for (i, cmd) in filtered.iter().enumerate() {
        if y >= area.bottom() {
            return;
        }
        let style = if i == app.palette_selected {
            t::selected_style()
        } else {
            Style::default()
        };
        put(
            buf,
            area.left(),
            y,
            cmd.name,
            style.fg(if i == app.palette_selected {
                t::BG
            } else {
                t::CYAN
            }),
        );
        let desc_style = Style::default().fg(if i == app.palette_selected {
            t::BG
        } else {
            t::DIM
        });
        put(
            buf,
            area.left() + 16,
            y,
            cmd.description,
            desc_style,
        );
        y += 1;
    }
    if filtered.is_empty() {
        put(
            buf,
            area.left(),
            y,
            &format!("(no command matches \"/{}\")", app.palette_query),
            Style::default().fg(t::ERROR),
        );
    }
}
