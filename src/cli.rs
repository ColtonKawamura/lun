//! Phase 3: core non-TUI CLI commands on top of the Phase 2 DB layer.
//!
//! Commands:
//! - `lun status` — project table + all-task table + summary line.
//! - `lun status <name|P-001>` — project overview, per-status counts, tasks,
//!   summary.
//! - `lun proj add task "<title>"` — interactive task creation (prompts for
//!   status, priority, assignee, commit message), writes a `CREATE`
//!   log entry, prints `Created task T-00N in project X` + `Committed: <msg>`.
//! - `lun task <key|title>` — task fields, labels, timestamps, attachments,
//!   links, log history.
//! - `lun log <project|task>` — commit-style history.
//!
//! Phase 4 adds Mac linking & attachments:
//! - `lun attach task <T-00N|title> /path/to/file` — copies repo files into
//!   `.lun/attachments/` (name collisions get `-2`, `-3`, ... suffixes);
//!   files OUTSIDE the repo require a `y/N` confirmation and are linked by
//!   absolute path without copying.
//! - `lun link <task|project> <key|title> "<label>" "<uri>"` — record a
//!   link (file path, URL, or custom URI such as `obsidian://...`).
//! - `lun open-link <task|project> <key|title> <label>` — look up a link
//!   and open it with macOS `open`.
//!
//! All data comes from `.lun/lun.db`; the markdown-like text produced here is
//! terminal rendering only and is never stored.
//!
//! Resolution rules (per docs/plan.md):
//! - exact match only; names with whitespace must be quoted argv;
//! - project keys (`P-00N`), task keys (`T-00N`), project names, and task
//!   titles are all exact;
//! - for `lun log`, a string matching a project name/key wins over a task;
//! - unknown or ambiguous targets produce a clear error and a nonzero exit
//!   (2 for usage/resolution failures, 1 for DB/IO failures).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::{Mutex, OnceLock};

use crate::db::Result;
use crate::db::{
    Attachment, AttachmentTarget, DbError, Link, LinkTarget, LogEntry, Lun, Pr, PrSpec, Project,
    Task, TaskListSpec, TaskSort, TaskSpec, TaskUpdateSpec,
};

const TASK_STATUSES: [&str; 5] = ["todo", "doing", "follow-up", "blocked", "done"];
const PROJECT_STATUSES: [&str; 2] = ["active", "inactive"];
const TASK_SORT_KEYS: [&str; 5] = ["key", "title", "status", "priority", "updated"];
const TOP_LEVEL_COMMANDS: [&str; 15] = [
    "init",
    "new",
    "add",
    "task",
    "proj",
    "project",
    "move",
    "attach",
    "log",
    "status",
    "link",
    "open-link",
    "open-uri",
    "pr",
    "help",
];
const PR_SUBCOMMANDS: [&str; 4] = ["new", "show", "ls", "merge"];
const TASK_SUBCOMMANDS: [&str; 6] = ["ls", "edit", "complete", "reopen", "archive", "delete"];

fn push_unique(out: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !out.iter().any(|v| v == &value) {
        out.push(value);
    }
}

fn extend_unique<'a>(out: &mut Vec<String>, values: impl IntoIterator<Item = &'a str>) {
    for value in values {
        push_unique(out, value);
    }
}

fn task_candidates(app: Option<&App>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(app) = app else {
        return out;
    };
    if let Ok(tasks) = app.lun.list_tasks() {
        for task in tasks {
            push_unique(&mut out, task.task_key);
            push_unique(&mut out, task.title);
        }
    }
    out
}

fn project_candidates(app: Option<&App>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(app) = app else {
        return out;
    };
    if let Ok(projects) = app.lun.list_projects() {
        for project in projects {
            push_unique(&mut out, project.project_key);
            push_unique(&mut out, project.name);
        }
    }
    out
}

fn completion_flags(tokens: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if tokens.is_empty() {
        extend_unique(&mut out, ["--help", "--version"]);
        return out;
    }
    match tokens[0].as_str() {
        "status" => extend_unique(&mut out, ["--board"]),
        "new" => extend_unique(&mut out, ["--status", "--message"]),
        "proj" | "project" if tokens.len() >= 2 => {
            extend_unique(&mut out, ["--status", "--message"]);
        }
        "proj" | "project" => {}
        "task" => match tokens.get(1).map(String::as_str) {
            Some("ls") => {
                extend_unique(
                    &mut out,
                    [
                        "--all",
                        "--project",
                        "--status",
                        "--priority",
                        "--assignee",
                        "--sort",
                    ],
                );
            }
            Some("edit") => {
                extend_unique(
                    &mut out,
                    [
                        "--title",
                        "--project",
                        "--status",
                        "--priority",
                        "--assignee",
                        "--branch",
                        "--labels",
                        "--notes",
                        "--message",
                    ],
                );
            }
            Some("reopen") => extend_unique(&mut out, ["--status", "--message"]),
            Some(_) => {}
            None => {}
        },
        "open-uri" => extend_unique(&mut out, ["--on"]),
        "pr" if tokens.get(1).map(String::as_str) == Some("new") => {
            extend_unique(&mut out, ["--from", "--to"]);
        }
        "pr" => {}
        _ => {}
    }
    out
}

fn completion_candidates(app: Option<&App>, words: &[String]) -> Vec<String> {
    let words = if words.first().map(String::as_str) == Some("lun") {
        &words[1..]
    } else {
        words
    };
    let current = words.last().map(String::as_str).unwrap_or("");
    let tokens = if words.is_empty() {
        &[][..]
    } else {
        &words[..words.len() - 1]
    };
    let mut out = Vec::new();

    if let Some(last) = tokens.last().map(String::as_str) {
        match last {
            "--status" => {
                let is_project = match tokens.first().map(String::as_str) {
                    Some("new") => {
                        matches!(tokens.get(1).map(String::as_str), Some("proj" | "project"))
                    }
                    Some("proj" | "project") => true,
                    _ => false,
                };
                if is_project {
                    extend_unique(&mut out, PROJECT_STATUSES);
                } else {
                    extend_unique(&mut out, TASK_STATUSES);
                }
            }
            "--sort" => extend_unique(&mut out, TASK_SORT_KEYS),
            "--project" => out.extend(project_candidates(app)),
            "--on" => extend_unique(&mut out, ["task", "project"]),
            "proj" | "project" => {
                if matches!(tokens.first().map(String::as_str), Some("add")) {
                    out.extend(project_candidates(app));
                }
            }
            "task"
                if matches!(
                    tokens.first().map(String::as_str),
                    Some("link" | "open-link" | "attach")
                ) || (tokens.first().map(String::as_str) == Some("open-uri")
                    && matches!(
                        tokens
                            .get(tokens.len().saturating_sub(2))
                            .map(String::as_str),
                        Some("--on")
                    )) =>
            {
                out.extend(task_candidates(app));
            }
            "task" => {}
            "status" => {
                out.extend(project_candidates(app));
                out.extend(task_candidates(app));
            }
            "log" => {
                out.extend(project_candidates(app));
                out.extend(task_candidates(app));
            }
            _ => {}
        }
    }

    if out.is_empty() {
        if words.len() <= 1 {
            extend_unique(&mut out, TOP_LEVEL_COMMANDS);
            extend_unique(&mut out, ["--help", "--version"]);
        } else {
            match tokens.first().map(String::as_str) {
                Some("new") if tokens.len() == 1 => extend_unique(&mut out, ["proj"]),
                Some("add") if tokens.len() == 1 => extend_unique(&mut out, ["task"]),
                Some("pr") if tokens.len() == 1 => extend_unique(&mut out, PR_SUBCOMMANDS),
                Some("task") if tokens.len() == 1 => {
                    extend_unique(&mut out, TASK_SUBCOMMANDS);
                    out.extend(task_candidates(app));
                }
                Some("task")
                    if matches!(
                        tokens.get(1).map(String::as_str),
                        Some("edit" | "complete" | "reopen" | "archive")
                    ) && tokens.len() == 2 =>
                {
                    out.extend(task_candidates(app));
                }
                Some("task")
                    if tokens.len() == 1 || (tokens.len() == 2 && !tokens[1].starts_with('-')) =>
                {
                    out.extend(task_candidates(app));
                }
                Some("status") if tokens.len() == 1 => {
                    out.extend(project_candidates(app));
                    out.extend(task_candidates(app));
                }
                Some("log") if tokens.len() == 1 => {
                    out.extend(project_candidates(app));
                    out.extend(task_candidates(app));
                }
                Some("pr")
                    if tokens.get(1).map(String::as_str) == Some("new") && tokens.len() == 2 =>
                {
                    out.extend(task_candidates(app));
                }
                Some("proj" | "project") if tokens.len() == 1 => {
                    out.extend(project_candidates(app))
                }
                Some("link" | "open-link" | "attach")
                    if matches!(tokens.get(1).map(String::as_str), Some("task"))
                        && tokens.len() == 2 =>
                {
                    out.extend(task_candidates(app));
                }
                Some("link" | "open-link" | "attach")
                    if matches!(tokens.get(1).map(String::as_str), Some("project"))
                        && tokens.len() == 2 =>
                {
                    out.extend(project_candidates(app));
                }
                Some("move") if tokens.len() == 1 => out.extend(task_candidates(app)),
                Some("move") if tokens.len() == 2 => out.extend(project_candidates(app)),
                Some("open-uri")
                    if tokens.get(1).map(String::as_str) == Some("--on")
                        && tokens.get(2).map(String::as_str) == Some("task")
                        && tokens.len() == 3 =>
                {
                    out.extend(task_candidates(app));
                }
                Some("open-uri")
                    if tokens.get(1).map(String::as_str) == Some("--on")
                        && tokens.get(2).map(String::as_str) == Some("project")
                        && tokens.len() == 3 =>
                {
                    out.extend(project_candidates(app));
                }
                _ => {}
            }
        }
    }

    if out.is_empty() && current.is_empty() && tokens.len() == 1 {
        let partial = tokens[0].as_str();
        if !partial.is_empty() && !TOP_LEVEL_COMMANDS.contains(&partial) {
            for cmd in TOP_LEVEL_COMMANDS {
                if cmd.starts_with(partial) {
                    push_unique(&mut out, cmd);
                }
            }
        }
    }

    if current.starts_with('-') {
        for flag in completion_flags(tokens) {
            push_unique(&mut out, flag);
        }
    }

    out.into_iter().filter(|v| v.starts_with(current)).collect()
}

pub fn complete_output(app: Option<&App>, args: &[String]) -> String {
    let words = if args.first().map(String::as_str) == Some("--") {
        &args[1..]
    } else {
        args
    };
    completion_candidates(app, words).join("\n")
}

/// CLI exit codes: 2 = usage/resolution error, 1 = runtime (DB/IO) error.
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_RUNTIME: u8 = 1;

pub fn cwd_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn ends_with_whitespace(input: &str) -> bool {
    input
        .chars()
        .last()
        .map(|c| c.is_whitespace())
        .unwrap_or(false)
}

fn tokenize_command_line(
    input: &str,
    allow_unclosed_quotes: bool,
) -> std::result::Result<(Vec<String>, bool), String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                } else {
                    cur.push('\\');
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
    if (in_single || in_double) && !allow_unclosed_quotes {
        return Err("unterminated quoted string".to_string());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok((out, ends_with_whitespace(input)))
}

pub fn split_command_line(input: &str) -> std::result::Result<Vec<String>, String> {
    tokenize_command_line(input, false).map(|(words, _)| words)
}

pub fn complete_line(app: Option<&App>, input: &str) -> Vec<String> {
    let (mut words, trailing_ws) = tokenize_command_line(input, true).unwrap_or_default();
    if trailing_ws {
        words.push(String::new());
    }
    completion_candidates(app, &words)
}

/// An opened lun database rooted at an explicit directory (tests) or the CWD
/// (the binary).
pub struct App {
    pub lun: Lun,
}

impl App {
    /// Open an existing, already-initialized DB under `root` (no migration
    /// side effects beyond what `Lun::open` does — it requires the file).
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            lun: Lun::open(root)?,
        })
    }
}

pub fn help_text(version: &str) -> String {
    format!(
        "lun v{version} — CLI-first markdown task & project tracker\n\n\
Usage:\n\
  lun                     Show banner\n\
  lun init                Create .lun/lun.db in the current directory (idempotent)\n\
  lun status [name|P-00N] [--board] Projects overview, or one project's tasks/board\n\
  lun new proj \"<name>\"  Create a project\n\
  lun add task \"<title>\" [proj \"<project>\"]  Create a task (interactive prompts)\n\
  lun move \"<task>\" \"<project>\"   Move a task to a project\n\
  lun task <T-00N|title>    View a task (fields, labels, history)\n\
  lun task <task> --status <todo|doing|follow-up|blocked|done>   Update task status\n\
  lun task ls [filters]     List tasks (--project/--status/--priority/--assignee/--sort/--all)\n\
  lun task edit <task> [--field value]   Edit task fields\n\
  lun proj <project> --status <active|inactive>   Update project status\n\
  lun task complete|reopen|archive <task>   Update task lifecycle\n\
  lun log <project|task>    Commit-style history for a project or task\n\
  lun attach <task|project> <key|title> /path/to/file   Attach a file (copies repo files into .lun/attachments/)\n\
  lun attach ls <task|project> <key|title>   List attachments\n\
  lun attach open|rm <task|project> <key|title> <filename|id>   Open/remove attachments\n\
  lun link <task|project> <key|title> \"<label>\" \"<uri>\"   Record a link\n\
  lun open-link <task|project> <key|title> <label>   Open a link via macOS `open`\n\
  lun open-uri <uri> [--on <task|project> <key|title>]   Open any URI (used by the nvim plugin; logs LINK_OPENED with --on)\n\
  lun pr new <T-00N|title> [--from <branch>] [--to <branch>]   Open a PR (defaults: task's branch -> main)\n\
  lun pr show <PR-00N|task>   View a PR (branches, status, PR log history)\n\
  lun pr ls               List open and merged PRs\n\
  lun pr merge <PR-00N|task>   Merge a PR (task -> done; runs `git merge` when possible)\n\
  lun --version             Print version\n\
  lun --help                Print this help\n\n\
Planned (later phases):\n\
  lun (no args in TUI mode) Full-screen TUI (Phase 5+)"
    )
}

pub fn init_db_command(app: &App) -> Result<String> {
    let version = app.lun.schema_version()?;
    let mut out = String::new();
    if version == crate::db::CURRENT_VERSION {
        out.push_str(&format!(
            "lun init: .lun/lun.db ready (schema v{version})\n"
        ));
    }
    out.push_str("lun init: re-run anytime — migrations are idempotent.");
    Ok(out)
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

fn is_project_key(s: &str) -> bool {
    let rest = s.strip_prefix("P-").unwrap_or("");
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn is_task_key(s: &str) -> bool {
    let rest = s.strip_prefix("T-").unwrap_or("");
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

/// Resolve a project query (for `lun status <...>`): exact key first, then
/// exact name. A task key is a clear, actionable "not found".
pub fn resolve_project(lun: &Lun, query: &str) -> Result<Project> {
    if is_project_key(query) {
        return lun.project_by_key(query);
    }
    if let Ok(p) = lun.project_by_name(query) {
        return Ok(p);
    }
    if is_task_key(query) {
        return Err(DbError::new(
            "not-found",
            format!("'{query}' is a task key — use `lun task {query}` or `lun log {query}`"),
        ));
    }
    Err(DbError::new(
        "not-found",
        format!(
            "unknown project '{query}': no project with this name or key (try `lun status` to list projects)"
        ),
    ))
}

/// Resolve a task query (for `lun task <...>`): exact key first, then exact
/// title; >1 title matches is ambiguous.
pub fn resolve_task(lun: &Lun, query: &str) -> Result<Task> {
    if is_task_key(query) {
        return lun.task_by_key(query);
    }
    let matches = lun.tasks_by_title(query)?;
    match matches.as_slice() {
        [] => Err(DbError::new(
            "not-found",
            format!(
                "unknown task '{query}': no task has this key or title (try `lun status`)"
            ),
        )),
        [t] => Ok(t.clone()),
        many => Err(DbError::new(
            "ambiguous",
            format!(
                "ambiguous task '{query}': {} tasks share this title: {} — use the task key (e.g. {})",
                many.len(),
                many.iter().map(|t| t.task_key.clone()).collect::<Vec<_>>().join(", "),
                many[0].task_key
            ),
        )),
    }
}

/// A resolved entity for `lun log <...>`.
#[derive(Debug)]
pub enum Entity {
    Project(Project),
    Task(Task),
}

/// Resolve `lun log`'s argument: project (key, then name) wins over task
/// (key, then title), per the plan's parsing rules. Matching is exact on
/// trimmed input (the shell strips surrounding quotes).
pub fn resolve_entity(lun: &Lun, query: &str) -> Result<Entity> {
    let q = query.trim();
    if is_project_key(q) {
        return lun
            .project_by_key(q)
            .map(Entity::Project)
            .map_err(|e| DbError::new("not-found", e.to_string()));
    }
    if is_task_key(q) {
        return lun
            .task_by_key(q)
            .map(Entity::Task)
            .map_err(|e| DbError::new("not-found", e.to_string()));
    }
    if let Ok(p) = lun.project_by_name(q) {
        return Ok(Entity::Project(p));
    }
    let matches = lun.tasks_by_title(q)?;
    match matches.as_slice() {
        [] => Err(DbError::new(
            "not-found",
            format!(
                "unknown project or task '{q}': no exact match (names with whitespace must be quoted)"
            ),
        )),
        [t] => Ok(Entity::Task(t.clone())),
        many => Err(DbError::new(
            "ambiguous",
            format!(
                "ambiguous '{q}': {} tasks share this title: {} — use the task key",
                many.len(),
                many.iter().map(|t| t.task_key.clone()).collect::<Vec<_>>().join(", ")
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Small rendering helpers
// ---------------------------------------------------------------------------

/// Pad cells to column widths, join with 3 spaces, trim trailing spaces.
fn pad_row(cells: &[String], widths: &[usize]) -> String {
    let mut s = String::new();
    for (i, c) in cells.iter().enumerate() {
        if i > 0 {
            s.push_str("   ");
        }
        let w = widths.get(i).map(|x| *x).unwrap_or(c.len());
        s.push_str(&format!("{:w$}", c, w = w));
    }
    s.trim_end().to_string()
}

/// Render a monospaced table: header row + data rows, 3-space column gaps.
pub fn render_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let ncols = headers.len();
    let widths: Vec<usize> = (0..ncols)
        .map(|i| {
            let mut w = headers[i].len();
            for r in rows {
                if i < r.len() {
                    w = w.max(r[i].len());
                }
            }
            w
        })
        .collect();
    let mut lines: Vec<String> = Vec::with_capacity(rows.len() + 1);
    let hcells: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
    lines.push(pad_row(&hcells, &widths));
    for r in rows {
        lines.push(pad_row(r, &widths));
    }
    lines.join("\n")
}

/// `2026-09-25T16:45:00Z` (UTC, as stored) -> `2026-09-25 16:45` (display).
/// `pub` so the TUI (Phase 6) renders timestamps identically to the CLI.
pub fn display_ts(ts: &str) -> String {
    let t = ts.replace('T', " ");
    t.chars().take(16).collect()
}

/// Pull a string value out of a small flat JSON object stored as TEXT
/// (e.g. `{"status": "todo", "priority": "med"}`). Details are written by
/// lun itself with string values only; this avoids a JSON dependency.
pub fn json_str(details: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = details.find(&needle)? + needle.len();
    let after = details[start..].find(':')?;
    let after = details[start + after + 1..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let body = &after[1..];
    let mut out = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(&n) = chars.peek() {
                    chars.next();
                    match n {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        other => {
                            out.push('\\');
                            out.push(other);
                        }
                    }
                }
            }
            '"' => break,
            c => out.push(c),
        }
    }
    Some(out)
}

/// Per-project task counts in board/status order.
fn status_counts(tasks: &[Task]) -> [usize; 5] {
    let mut counts = [0; 5];
    for task in tasks {
        if let Some(i) = TASK_STATUSES.iter().position(|s| *s == task.status) {
            counts[i] += 1;
        }
    }
    counts
}

fn counts_by_status(tasks: &[Task]) -> Vec<(&'static str, usize)> {
    let mut out = Vec::new();
    for s in TASK_STATUSES {
        out.push((s, tasks.iter().filter(|t| t.status == s).count()));
    }
    out
}

/// Summary line with all task statuses.
pub fn summary_line(n_projects: usize, tasks: &[Task]) -> String {
    let mut counts = [0; 5];
    for task in tasks {
        if let Some(i) = TASK_STATUSES.iter().position(|s| *s == task.status) {
            counts[i] += 1;
        }
    }
    format!(
        "Summary: {n_projects} project{} · {} task{} ({} todo, {} doing, {} follow-up, {} blocked, {} done)",
        if n_projects == 1 { "" } else { "s" },
        tasks.len(),
        if tasks.len() == 1 { "" } else { "s" },
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4]
    )
}

fn task_row(lun: &Lun, t: &Task, with_project: bool) -> Vec<String> {
    let mut cells = vec![t.task_key.clone()];
    if with_project {
        cells.push(lun.project_name_for_task(t));
    }
    cells.push(t.title.clone());
    cells.push(t.status.clone());
    cells.push(t.priority.clone());
    cells.push(t.assignee.clone().unwrap_or_default());
    cells.push(t.branch.clone().unwrap_or_default());
    cells
}

/// One compact detail line (or none) for a log entry, used where the plan
/// shows a single `Status: ...` line under the entry.
pub(crate) fn entry_inline_details(entry: &LogEntry) -> Option<String> {
    if let Some(changes) = json_str(&entry.details, "changes") {
        let parts: Vec<String> = changes
            .split(", ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if !parts.is_empty() {
            return Some(parts.join(", "));
        }
    }
    let status = json_str(&entry.details, "status");
    let priority = json_str(&entry.details, "priority");
    match (status, priority) {
        (Some(s), Some(p)) if entry.entity_type == "task" => {
            Some(format!("Status: {s}, Priority: {p}"))
        }
        (Some(s), _) => Some(format!("Status: {s}")),
        _ => None,
    }
}

/// History lines for `lun task` (plan "Viewing a task" format):
/// `- <ts>  <user>  <ACTION>` + compact detail + `Commit:` (COMMENT entries
/// carry a `Note:` instead). `pub` so the TUI's task view (Phase 6) renders
/// the identical history.
pub fn task_view_entry_lines(entry: &LogEntry) -> Vec<String> {
    let mut lines = vec![format!(
        "- {}  {}  {}",
        display_ts(&entry.timestamp),
        entry.user,
        entry.action
    )];
    if let Some(d) = entry_inline_details(entry) {
        lines.push(format!("    {d}"));
    }
    if let Some(note) = json_str(&entry.details, "note") {
        lines.push(format!("    Note: \"{note}\""));
    }
    if let Some(filename) = json_str(&entry.details, "filename") {
        lines.push(format!("    File: {filename}"));
    }
    if let Some(label) = json_str(&entry.details, "label") {
        if let Some(uri) = json_str(&entry.details, "uri") {
            lines.push(format!("    Link: [{label}] {uri}"));
        }
    }
    if entry.action != "COMMENT" && json_str(&entry.details, "note").is_none() {
        lines.push(format!("    Commit: {}", entry.message));
    }
    lines
}

/// Full detail lines for `lun log <task>` (plan "Logs for tasks" format):
/// no bullet; CREATE expands Project/Status/Priority, UPDATE a
/// `Field changes:` block, COMMENT a note, ATTACH the filename; most end
/// with the `Commit:` line. `pub` so the TUI's log view (Phase 6) renders
/// the identical lines.
pub fn task_log_entry_lines(entry: &LogEntry) -> Vec<String> {
    let mut lines = vec![format!(
        "{}  {}  {}",
        display_ts(&entry.timestamp),
        entry.user,
        entry.action
    )];
    if let Some(project) = json_str(&entry.details, "project") {
        lines.push(format!("    Project: {project}"));
    }
    match (
        json_str(&entry.details, "status"),
        json_str(&entry.details, "priority"),
    ) {
        (Some(s), Some(p)) => {
            lines.push(format!("    Status:  {s}"));
            lines.push(format!("    Priority: {p}"));
        }
        (Some(s), None) => lines.push(format!("    Status:  {s}")),
        _ => {}
    }
    if let Some(changes) = json_str(&entry.details, "changes") {
        lines.push("    Field changes:".into());
        for part in changes.split(", ").filter(|c| !c.is_empty()) {
            lines.push(format!("      {part}"));
        }
    }
    if let Some(note) = json_str(&entry.details, "note") {
        lines.push(format!("    Note: \"{note}\""));
    }
    if let Some(filename) = json_str(&entry.details, "filename") {
        lines.push(format!("    File: {filename}"));
    }
    if let Some(label) = json_str(&entry.details, "label") {
        if let Some(uri) = json_str(&entry.details, "uri") {
            lines.push(format!("    Link: [{label}] {uri}"));
        }
    }
    if entry.action != "COMMENT" && json_str(&entry.details, "note").is_none() {
        lines.push(format!("    Commit: {}", entry.message));
    }
    lines
}

/// Lines for `lun log <project>` (plan "Logs for projects" format): the
/// commit message IS the line, with a compact detail line under it.
/// `pub` so the TUI's log view (Phase 6) renders the identical lines.
pub fn project_log_entry_lines(entry: &LogEntry) -> Vec<String> {
    let mut lines = vec![format!(
        "{}  {}  {}",
        display_ts(&entry.timestamp),
        entry.user,
        entry.message
    )];
    if let Some(d) = entry_inline_details(entry) {
        lines.push(format!("    {d}"));
    }
    if let Some(note) = json_str(&entry.details, "note") {
        lines.push(format!("    Note: \"{note}\""));
    }
    if let Some(filename) = json_str(&entry.details, "filename") {
        lines.push(format!("    File: {filename}"));
    }
    if let Some(label) = json_str(&entry.details, "label") {
        if let Some(uri) = json_str(&entry.details, "uri") {
            lines.push(format!("    Link: [{label}] {uri}"));
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// Views (pure: App in -> markdown-ish String out)
// ---------------------------------------------------------------------------

fn status_name(status: &str) -> &'static str {
    match status {
        "todo" => "Todo",
        "doing" => "Doing",
        "follow-up" => "Follow-Up",
        "blocked" => "Blocked",
        "done" => "Done",
        _ => "?",
    }
}

/// `lun status` — global project table and summary.
pub fn status_all(app: &App) -> Result<String> {
    let projects = app.lun.list_projects()?;
    let tasks = app.lun.list_tasks()?;

    let mut prow = Vec::new();
    for p in &projects {
        let counts = status_counts(&app.lun.tasks_for_project(p.id)?);
        prow.push(vec![
            p.project_key.clone(),
            p.name.clone(),
            p.status.clone(),
            counts[0].to_string(),
            counts[1].to_string(),
            counts[2].to_string(),
            counts[3].to_string(),
            counts[4].to_string(),
        ]);
    }

    let mut out = String::new();
    out.push_str("Projects\n--------\n\n");
    out.push_str(&render_table(
        &[
            "ID",
            "Name",
            "Status",
            "Todo",
            "Doing",
            "Follow-Up",
            "Blocked",
            "Done",
        ],
        &prow,
    ));
    out.push('\n');
    out.push_str(&summary_line(projects.len(), &tasks));
    Ok(out)
}

/// `lun status <name|P-001>` — project overview, tasks-by-status, tasks,
/// summary.
pub fn status_project(app: &App, query: &str) -> Result<String> {
    let p = resolve_project(&app.lun, query)?;
    let tasks = app.lun.tasks_for_project(p.id)?;

    let header = format!("Project: {}", p.name);
    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    out.push_str(&"=".repeat(header.chars().count()));
    out.push_str("\n\nOverview\n--------\n\n");
    out.push_str(&format!("ID:      {}\n", p.project_key));
    out.push_str(&format!("Name:    {}\n", p.name));
    out.push_str(&format!("Status:  {}\n\n", p.status));
    out.push_str("Tasks by Status:\n");
    for (s, n) in counts_by_status(&tasks) {
        out.push_str(&format!("- {:<14}{}\n", format!("{s}:"), n));
    }
    out.push_str("\nTasks\n-----\n\n");
    let trow: Vec<Vec<String>> = tasks.iter().map(|t| task_row(&app.lun, t, true)).collect();
    out.push_str(&render_table(
        &[
            "ID", "Project", "Title", "Status", "Priority", "Assignee", "Branch",
        ],
        &trow,
    ));
    out.push('\n');
    out.push_str(&summary_line(1, &tasks));
    Ok(out)
}

pub fn status_project_board(app: &App, query: &str) -> Result<String> {
    let p = resolve_project(&app.lun, query)?;
    let tasks = app.lun.tasks_for_project(p.id)?;
    let header = format!("Board: {}", p.name);
    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    out.push_str(&"=".repeat(header.chars().count()));
    out.push_str("\n\n");
    for status in TASK_STATUSES {
        out.push_str(&format!(
            "{}\n{}\n",
            status_name(status),
            "-".repeat(status_name(status).len())
        ));
        let items: Vec<&Task> = tasks.iter().filter(|t| t.status == status).collect();
        if items.is_empty() {
            out.push_str("- (none)\n\n");
            continue;
        }
        for task in items {
            out.push_str(&format!("- {}  {}\n", task.task_key, task.title));
        }
        out.push('\n');
    }
    out.push_str(&summary_line(1, &tasks));
    Ok(out.trim_end().to_string())
}

pub fn status_target(app: &App, query: &str) -> Result<String> {
    match resolve_entity(&app.lun, query)? {
        Entity::Project(p) => status_project(app, &p.project_key),
        Entity::Task(t) => status_task(app, &t.task_key),
    }
}

pub fn status_task(app: &App, query: &str) -> Result<String> {
    let t = resolve_task(&app.lun, query)?;
    let project = app.lun.project_name_for_task(&t);
    let last = app.lun.logs_for("task", t.id)?.into_iter().next();

    let mut out = String::new();
    out.push_str(&format!("**Task {}**\n\n", t.task_key));
    out.push_str("==========\n\n");
    out.push_str(&format!("Project:   {project}\n"));
    out.push_str(&format!("Title:     {}\n", t.title));
    out.push_str(&format!("Status:    {}\n", t.status));
    out.push_str(&format!("Priority:  {}\n", t.priority));
    out.push_str(&format!("Assignee:  {}\n", t.assignee.unwrap_or_default()));
    out.push_str(&format!("Branch:    {}\n", t.branch.unwrap_or_default()));
    out.push_str(&format!("Created:   {}\n\n", display_ts(&t.created_at)));
    out.push_str("Checklist:\n\n");
    out.push_str("- [ ] (checklist editing arrives in a later phase)\n\n");
    out.push_str("**Notes:**\n\n");
    if t.notes.trim().is_empty() {
        out.push_str("- (add notes with 'e' in the task view)\n\n");
    } else {
        for line in t.notes.lines() {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }
    out.push_str("**Attachments:**\n\n");
    let attachments = app.lun.attachments_for_task(t.id)?;
    if attachments.is_empty() {
        out.push_str("- (drag a file onto the TUI to attach one)\n\n");
    } else {
        for a in &attachments {
            out.push_str(&format!("- {} ({})\n", a.filename, a.stored_path));
        }
        out.push('\n');
    }
    out.push_str("**Links:**\n\n");
    let links = app.lun.links_for_task(t.id)?;
    if links.is_empty() {
        out.push_str("- (none)\n\n");
    } else {
        for l in &links {
            out.push_str(&format!("- [{}] {}\n", l.label, l.uri));
        }
        out.push('\n');
    }
    out.push_str("**Last Commit:**\n");
    if let Some(entry) = last {
        for line in task_view_entry_lines(&entry) {
            out.push_str(&format!("{line}\n"));
        }
    } else {
        out.push_str("- (none)\n");
    }
    Ok(out)
}

/// `lun task <key|title>` — fields, labels, timestamps, log history.
pub fn task_view(app: &App, query: &str) -> Result<String> {
    let t = resolve_task(&app.lun, query)?;
    let project = app.lun.project_name_for_task(&t);
    let entries = app.lun.logs_for("task", t.id)?;

    let header = format!("Task {}", t.task_key);
    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    out.push_str(&"=".repeat(header.chars().count()));
    out.push_str("\n\n");
    out.push_str(&format!("Project:   {project}\n"));
    out.push_str(&format!("Title:     {}\n", t.title));
    out.push_str(&format!("Status:    {}\n", t.status));
    out.push_str(&format!("Priority:  {}\n", t.priority));
    out.push_str(&format!("Assignee:  {}\n", t.assignee.unwrap_or_default()));
    out.push_str(&format!("Labels:    {}\n", t.labels));
    out.push_str(&format!("Branch:    {}\n", t.branch.unwrap_or_default()));
    out.push_str(&format!(
        "Archived:  {}\n",
        if t.archived_at.is_some() { "yes" } else { "no" }
    ));
    out.push_str(&format!("Created:   {}\n", display_ts(&t.created_at)));
    out.push_str(&format!("Updated:   {}\n", display_ts(&t.updated_at)));
    out.push_str("\nChecklist:\n");
    out.push_str(&format!(
        "- [ ] (add checklist items with `lun task edit {}`)",
        t.task_key
    ));
    out.push_str("\nNotes:\n");
    // Phase 7: notes are real data (TUI-editable). Show the saved text
    // when present; otherwise the add-hint (editing still lands with a
    // later CLI phase — the TUI edits them today).
    if t.notes.trim().is_empty() {
        out.push_str(&format!(
            "- (add notes with `lun task edit {}`)",
            t.task_key
        ));
    } else {
        for line in t.notes.lines() {
            out.push_str(&format!("- {line}\n"));
        }
    }
    out.push('\n');
    out.push_str("\nAttachments:\n");
    let attachments = app.lun.attachments_for_task(t.id)?;
    if attachments.is_empty() {
        out.push_str(&format!(
            "- (no attachments - add one with `lun attach task {t} /path/to/file`)",
            t = t.task_key
        ));
    } else {
        for a in &attachments {
            out.push_str(&format!("- {} ({})\n", a.filename, a.stored_path));
        }
    }
    out.push_str("\nLinks:\n");
    let links = app.lun.links_for_task(t.id)?;
    if links.is_empty() {
        out.push_str(&format!(
            "- (no links - add one with `lun link task {t} <label> <uri>`)",
            t = t.task_key
        ));
    } else {
        for l in &links {
            out.push_str(&format!("- [{}] {}\n", l.label, l.uri));
        }
    }
    out.push_str("\nHistory (log):\n");
    for e in &entries {
        for line in task_view_entry_lines(e) {
            out.push_str(&format!("{line}\n"));
        }
    }
    out.push_str("\nPRs:\n");
    let prs = app.lun.prs_for_task(t.id)?;
    if prs.is_empty() {
        out.push_str(&format!(
            "- (no PRs - open one with `lun pr new {} --from <branch> --to main`)",
            t.task_key
        ));
    } else {
        for pr in &prs {
            out.push_str(&format!(
                "- {}  {} -> {}  [{}]  opened {}",
                pr.pr_key,
                pr.source_branch,
                pr.target_branch,
                pr.status,
                display_ts(&pr.created_at)
            ));
            if let Some(merged) = &pr.merged_at {
                out.push_str(&format!("  merged {}", display_ts(merged)));
            }
            out.push('\n');
        }
    }
    Ok(out)
}

/// `lun log <project|task>` — commit-style history, newest first.
pub fn log_view(app: &App, query: &str) -> Result<String> {
    let entity = resolve_entity(&app.lun, query)?;
    let entries: Vec<LogEntry> = match entity {
        Entity::Project(p) => {
            let mut all = app.lun.logs_for("project", p.id)?;
            all.extend(app.lun.logs_for_project(p.id)?);
            all.sort_by(|a, b| b.id.cmp(&a.id));
            all
        }
        Entity::Task(t) => app.lun.logs_for("task", t.id)?,
    };

    let mut out = String::new();
    for e in &entries {
        let lines = task_view_entry_lines(e);
        for line in lines {
            out.push_str(&format!("{line}\n"));
        }
        out.push('\n');
    }
    Ok(out.trim_end().to_string())
}

// ---------------------------------------------------------------------------
// Phase 4: attachments & links (Mac linking)
// ---------------------------------------------------------------------------

/// Where copied attachments live: `<root>/.lun/attachments/`.
pub fn attachments_root(root: &Path) -> PathBuf {
    root.join(".lun/attachments")
}

/// Copy `src` into `<root>/.lun/attachments/`, suffixing the file name
/// (`-2`, `-3`, ...) on collision. Returns the destination path.
/// `pub(crate)` so the TUI's drop/paste attach (Phase 7) reuses the exact
/// same collision-suffixed copy semantics.
pub(crate) fn copy_into_attachments(root: &Path, src: &Path) -> Result<PathBuf> {
    let dir = attachments_root(root);
    std::fs::create_dir_all(&dir)
        .map_err(|e| DbError::new("io", format!("creating {}: {e}", dir.display())))?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| DbError::new("invalid", format!("'{}' has no file name", src.display())))?;
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), Some(name[i..].to_string())),
        _ => (name.clone(), None),
    };
    let mut dest = dir.join(&name);
    let mut n = 2;
    while dest.exists() {
        let suffixed = match &ext {
            Some(e) => format!("{stem}-{n}{e}"),
            None => format!("{stem}-{n}"),
        };
        dest = dir.join(suffixed);
        n += 1;
    }
    std::fs::copy(src, &dest).map_err(|e| {
        DbError::new(
            "io",
            format!("copying {} to {}: {e}", src.display(), dest.display()),
        )
    })?;
    Ok(dest)
}

fn resolve_attachment_target(
    app: &App,
    kind: &str,
    query: &str,
) -> Result<(AttachmentTarget, String)> {
    match kind {
        "task" => {
            let task = resolve_task(&app.lun, query)?;
            Ok((AttachmentTarget::Task(task.id), task.task_key))
        }
        "project" => {
            let project = resolve_project(&app.lun, query)?;
            Ok((AttachmentTarget::Project(project.id), project.project_key))
        }
        _ => Err(DbError::new("usage", "expected <task|project> target")),
    }
}

fn validate_open_target(target: &str) -> Result<()> {
    if target.trim().is_empty() {
        return Err(DbError::new("usage", "open target must not be empty"));
    }
    if target.chars().any(|c| c.is_control()) {
        return Err(DbError::new(
            "invalid",
            "open target contains control characters",
        ));
    }
    Ok(())
}

pub fn open_target(target: &str) -> Result<()> {
    validate_open_target(target)?;
    let opener = std::env::var("LUN_OPEN_BIN").unwrap_or_else(|_| "open".to_string());
    let status = Command::new(&opener)
        .arg(target)
        .status()
        .map_err(|e| DbError::new("io", format!("spawning `{opener}`: {e}")))?;
    if !status.success() {
        return Err(DbError::new(
            "io",
            format!("`{opener} {target}` exited with {status}"),
        ));
    }
    Ok(())
}

fn find_attachment(app: &App, kind: &str, query: &str, needle: &str) -> Result<Attachment> {
    let attachments = match kind {
        "task" => app
            .lun
            .attachments_for_task(resolve_task(&app.lun, query)?.id)?,
        "project" => app
            .lun
            .attachments_for_project(resolve_project(&app.lun, query)?.id)?,
        _ => {
            return Err(DbError::new(
                "usage",
                "expected: lun attach <task|project> ...",
            ))
        }
    };
    let matches: Vec<Attachment> = attachments
        .into_iter()
        .filter(|a| a.filename == needle.trim() || a.id.to_string() == needle.trim())
        .collect();
    match matches.as_slice() {
        [attachment] => Ok(attachment.clone()),
        [] => Err(DbError::new(
            "not-found",
            format!("no attachment '{needle}' on {kind} '{query}'"),
        )),
        many => Err(DbError::new(
            "ambiguous",
            format!("ambiguous attachment '{needle}': {} matches", many.len()),
        )),
    }
}

/// `lun attach task <key|title> /path/to/file`.
///
/// Files inside `root` are copied into `.lun/attachments/`; files OUTSIDE
/// `root` require `y` at the confirmation prompt (anything else aborts) and
/// are recorded by absolute path without copying.
pub fn attach_file(
    app: &App,
    root: &Path,
    task_query: &str,
    file_path: &str,
    stdin: &mut dyn BufRead,
) -> Result<String> {
    attach_entity_file(app, root, "task", task_query, file_path, stdin)
}

pub fn attach_entity_file(
    app: &App,
    root: &Path,
    kind: &str,
    task_query: &str,
    file_path: &str,
    stdin: &mut dyn BufRead,
) -> Result<String> {
    let (target, key) = resolve_attachment_target(app, kind, task_query)?;
    let src = Path::new(file_path);
    if !src.is_file() {
        return Err(DbError::new(
            "not-found",
            format!("no such file: {} (expected a path to a file)", file_path),
        ));
    }

    let inside = std::path::absolute(src)
        .ok()
        .zip(std::path::absolute(root).ok())
        .map(|(s, r)| s.starts_with(&r))
        .unwrap_or(false);

    let stored = if inside {
        copy_into_attachments(root, src)?
    } else {
        let answer = read_prompt(
            stdin,
            "This path is outside the current repo. Link anyway? [y/N] ",
        )?;
        if !matches!(answer.to_lowercase().as_str(), "y" | "yes") {
            return Err(DbError::new(
                "declined",
                format!(
                    "not attached: '{}' is outside {} (answer 'y' to link it by path anyway)",
                    file_path,
                    root.display()
                ),
            ));
        }
        std::path::absolute(src).map_err(|e| DbError::new("io", format!("resolving path: {e}")))?
    };

    let filename = stored
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    app.lun.add_attachment_to(
        target,
        &filename,
        stored.to_str().unwrap_or_default(),
        None,
        None,
    )?;
    Ok(format!(
        "Attached {} to {} (stored: {})",
        filename,
        key,
        stored.display()
    ))
}

/// `lun link <task|project> <key|title> "<label>" "<uri>"`.
pub fn add_link_command(
    app: &App,
    kind: &str,
    query: &str,
    label: &str,
    uri: &str,
) -> Result<String> {
    if label.trim().is_empty() {
        return Err(DbError::new("usage", "link label must not be empty"));
    }
    if uri.trim().is_empty() {
        return Err(DbError::new("usage", "link URI must not be empty"));
    }
    let (target, label2) = match kind {
        "task" => {
            let t = resolve_task(&app.lun, query)?;
            (LinkTarget::Task(t.id), format!("task {}", t.task_key))
        }
        "project" => {
            let p = resolve_project(&app.lun, query)?;
            (
                LinkTarget::Project(p.id),
                format!("project {} [{}]", p.name, p.project_key),
            )
        }
        _ => {
            return Err(DbError::new(
                "usage",
                "expected: lun link <task|project> <key|title> \"<label>\" \"<uri>\"",
            ))
        }
    };
    app.lun.add_link(target, label, uri, None, None)?;
    Ok(format!("Linked {label} to {label2}: {uri}"))
}

/// Look up a link by label on a task or project; returns the URI.
pub fn resolve_link(app: &App, kind: &str, query: &str, label: &str) -> Result<String> {
    let links = match kind {
        "task" => app.lun.links_for_task(resolve_task(&app.lun, query)?.id)?,
        "project" => app
            .lun
            .links_for_project(resolve_project(&app.lun, query)?.id)?,
        _ => {
            return Err(DbError::new(
                "usage",
                "expected: lun open-link <task|project> <key|title> <label>",
            ))
        }
    };
    let matches: Vec<Link> = links
        .into_iter()
        .filter(|l| l.label == label.trim())
        .collect();
    match matches.as_slice() {
        [l] => Ok(l.uri.clone()),
        [] => Err(DbError::new(
            "not-found",
            format!("no link labeled '{label}' on {kind} '{query}' (see `lun task {query}`)"),
        )),
        many => Err(DbError::new(
            "ambiguous",
            format!(
                "ambiguous link '{label}' on {kind} '{query}': {} entries",
                many.len()
            ),
        )),
    }
}

/// `lun open-link <task|project> <key|title> <label>` — resolve the link and
/// hand the URI to macOS `open`.
pub fn open_link(app: &App, kind: &str, query: &str, label: &str) -> Result<String> {
    let uri = resolve_link(app, kind, query, label)?;
    open_target(&uri)?;
    Ok(format!("Opened: {uri}"))
}

/// `lun open-uri <uri> [--on task|project <key|title>]` — hand an arbitrary
/// URI to macOS `open` (Phase 8: the nvim plugin's ⌘⇧L goes through this,
/// so the open path — and the optional LINK_OPENED log — live in lun, not
/// in the plugin).
///
/// With `--on <kind> <key|title>`, a `LINK_OPENED` log entry is written on
/// that entity (message `open link "<uri>"`, details `{"uri": ...}`).
pub fn open_uri(app: &App, args: &[String]) -> Result<String> {
    let mut uri: Option<&str> = None;
    let mut on: Option<(String, String)> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--on" => {
                let kind = args
                    .get(i + 1)
                    .ok_or_else(|| DbError::new("usage", "`--on` needs <task|project>"))?
                    .clone();
                if kind != "task" && kind != "project" {
                    return Err(DbError::new(
                        "usage",
                        format!("invalid --on kind '{kind}' (expected task or project)"),
                    ));
                }
                let q = args.get(i + 2).ok_or_else(|| {
                    DbError::new("usage", "`--on <task|project>` needs a <key|title>")
                })?;
                on = Some((kind, q.clone()));
                i += 3;
            }
            other if uri.is_none() => {
                uri = Some(other);
                i += 1;
            }
            other => {
                return Err(DbError::new(
                    "usage",
                    format!("unexpected argument '{other}' (expected: lun open-uri <uri> [--on <task|project> <key|title>])"),
                ));
            }
        }
    }
    let Some(uri) = uri else {
        return Err(DbError::new(
            "usage",
            "expected: lun open-uri <uri> [--on <task|project> <key|title>]",
        ));
    };

    open_target(uri)?;

    match on {
        Some((kind, q)) => {
            let (entity_type, entity_id, key) = match kind.as_str() {
                "task" => {
                    let t = resolve_task(&app.lun, &q)?;
                    ("task", t.id, t.task_key)
                }
                "project" => {
                    let p = resolve_project(&app.lun, &q)?;
                    ("project", p.id, p.project_key)
                }
                _ => unreachable!("kind validated above"),
            };
            app.lun.log(
                entity_type,
                entity_id,
                "LINK_OPENED",
                &format!("open link \"{uri}\""),
                &format!("{{\"uri\": \"{uri}\"}}"),
                None,
            )?;
            Ok(format!("Opened: {uri} (logged LINK_OPENED on {key})"))
        }
        None => Ok(format!("Opened: {uri}")),
    }
}

pub fn list_attachments(app: &App, kind: &str, query: &str) -> Result<String> {
    let (label, attachments) = match kind {
        "task" => {
            let task = resolve_task(&app.lun, query)?;
            (task.task_key, app.lun.attachments_for_task(task.id)?)
        }
        "project" => {
            let project = resolve_project(&app.lun, query)?;
            (
                project.project_key,
                app.lun.attachments_for_project(project.id)?,
            )
        }
        _ => {
            return Err(DbError::new(
                "usage",
                "expected: lun attach ls <task|project> <key|title>",
            ))
        }
    };
    let mut out = format!(
        "Attachments for {label}\n{}\n",
        "=".repeat(16 + label.len())
    );
    if attachments.is_empty() {
        out.push_str("- (none)");
        return Ok(out);
    }
    for attachment in attachments {
        out.push_str(&format!(
            "- [{}] {} ({})\n",
            attachment.id, attachment.filename, attachment.stored_path
        ));
    }
    Ok(out.trim_end().to_string())
}

pub fn remove_attachment_command(
    app: &App,
    kind: &str,
    query: &str,
    needle: &str,
) -> Result<String> {
    let attachment = find_attachment(app, kind, query, needle)?;
    let removed = app.lun.remove_attachment(attachment.id, None, None)?;
    let stored_path = Path::new(&removed.stored_path);
    let managed_copy = removed.stored_path.contains("/.lun/attachments/")
        || removed.stored_path.starts_with(".lun/attachments/");
    if managed_copy || stored_path.starts_with(attachments_root(Path::new("."))) {
        let _ = std::fs::remove_file(&removed.stored_path);
    }
    Ok(format!(
        "Removed attachment {} ({})",
        removed.filename, removed.stored_path
    ))
}

pub fn open_attachment_command(app: &App, kind: &str, query: &str, needle: &str) -> Result<String> {
    let attachment = find_attachment(app, kind, query, needle)?;
    open_target(&attachment.stored_path)?;
    Ok(format!("Opened: {}", attachment.stored_path))
}

fn parse_sort(value: &str) -> Result<TaskSort> {
    match value {
        "key" => Ok(TaskSort::Key),
        "title" => Ok(TaskSort::Title),
        "status" => Ok(TaskSort::Status),
        "priority" => Ok(TaskSort::Priority),
        "updated" => Ok(TaskSort::Updated),
        other => Err(DbError::new(
            "usage",
            format!("invalid --sort '{other}' (expected key, title, status, priority, updated)"),
        )),
    }
}

pub fn task_list(app: &App, args: &[String]) -> Result<String> {
    let mut spec = TaskListSpec::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" => {
                spec.include_archived = true;
                i += 1;
            }
            "--project" => {
                let q = args
                    .get(i + 1)
                    .ok_or_else(|| DbError::new("usage", "--project needs a value"))?;
                spec.project_id = Some(resolve_project(&app.lun, q)?.id);
                i += 2;
            }
            "--status" => {
                spec.status = Some(
                    args.get(i + 1)
                        .ok_or_else(|| DbError::new("usage", "--status needs a value"))?
                        .clone(),
                );
                i += 2;
            }
            "--priority" => {
                spec.priority = Some(
                    args.get(i + 1)
                        .ok_or_else(|| DbError::new("usage", "--priority needs a value"))?
                        .clone(),
                );
                i += 2;
            }
            "--assignee" => {
                spec.assignee = Some(
                    args.get(i + 1)
                        .ok_or_else(|| DbError::new("usage", "--assignee needs a value"))?
                        .clone(),
                );
                i += 2;
            }
            "--sort" => {
                spec.sort = parse_sort(
                    args.get(i + 1)
                        .ok_or_else(|| DbError::new("usage", "--sort needs a value"))?,
                )?;
                i += 2;
            }
            other => {
                return Err(DbError::new(
                    "usage",
                    format!("unexpected argument '{other}' (expected task ls filters)"),
                ))
            }
        }
    }
    let tasks = app.lun.list_tasks_with(&spec)?;
    let rows: Vec<Vec<String>> = tasks.iter().map(|t| task_row(&app.lun, t, true)).collect();
    let mut out = String::new();
    out.push_str("Tasks\n=====\n\n");
    out.push_str(&render_table(
        &[
            "ID", "Project", "Title", "Status", "Priority", "Assignee", "Branch",
        ],
        &rows,
    ));
    if !tasks.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("\nTotal: {}", tasks.len()));
    Ok(out)
}

pub fn task_edit(
    app: &App,
    query: &str,
    args: &[String],
    stdin: &mut dyn BufRead,
) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let mut spec = TaskUpdateSpec::default();
    let mut explicit_message: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let value = |i: usize, flag: &str, args: &[String]| {
            args.get(i + 1)
                .cloned()
                .ok_or_else(|| DbError::new("usage", format!("{flag} needs a value")))
        };
        match args[i].as_str() {
            "--title" => {
                spec.title = Some(value(i, "--title", args)?);
                i += 2;
            }
            "--project" => {
                spec.project_id =
                    Some(resolve_project(&app.lun, &value(i, "--project", args)?)?.id);
                i += 2;
            }
            "--status" => {
                spec.status = Some(value(i, "--status", args)?);
                i += 2;
            }
            "--priority" => {
                spec.priority = Some(value(i, "--priority", args)?);
                i += 2;
            }
            "--assignee" => {
                let value = value(i, "--assignee", args)?;
                spec.assignee = Some(if value == "none" { None } else { Some(value) });
                i += 2;
            }
            "--branch" => {
                let value = value(i, "--branch", args)?;
                spec.branch = Some(if value == "none" { None } else { Some(value) });
                i += 2;
            }
            "--labels" => {
                let value = value(i, "--labels", args)?;
                spec.labels = Some(if value == "none" {
                    "[]".to_string()
                } else {
                    let labels: Vec<String> = value
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("\"{}\"", s))
                        .collect();
                    format!("[{}]", labels.join(", "))
                });
                i += 2;
            }
            "--notes" => {
                spec.notes = Some(value(i, "--notes", args)?);
                i += 2;
            }
            "--message" => {
                explicit_message = Some(value(i, "--message", args)?);
                i += 2;
            }
            other => {
                return Err(DbError::new(
                    "usage",
                    format!("unexpected flag '{other}' for task edit"),
                ))
            }
        }
    }
    let default_message = format!("edit {}", task.task_key);
    spec.message = Some(read_commit_message_with_optional_override(
        stdin,
        explicit_message,
        &default_message,
    )?);
    let updated = app.lun.update_task(task.id, spec)?;
    Ok(format!(
        "Updated {}: {} [{} / {}]",
        updated.task_key, updated.title, updated.status, updated.priority
    ))
}

pub fn task_complete(app: &App, query: &str, stdin: &mut dyn BufRead) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let default = format!("complete {}", task.task_key);
    let message = read_commit_message(stdin, &default)?;
    let updated = app.lun.complete_task(task.id, Some(&message), None)?;
    Ok(format!(
        "Completed {} ({})\nCommitted: {}",
        updated.task_key, updated.title, message
    ))
}

pub fn task_reopen(
    app: &App,
    query: &str,
    args: &[String],
    stdin: &mut dyn BufRead,
) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let mut status: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--status" => {
                let raw = args
                    .get(i + 1)
                    .ok_or_else(|| DbError::new("usage", "--status needs a value"))?;
                status = Some(resolve_task_status(raw).ok_or_else(|| {
                    DbError::new(
                        "invalid",
                        format!(
                            "invalid status '{raw}' (expected todo, doing, follow-up, blocked, or done)"
                        ),
                    )
                })?);
                i += 2;
            }
            other => {
                return Err(DbError::new(
                    "usage",
                    format!("unexpected argument '{other}' for task reopen"),
                ))
            }
        }
    }
    let target = status.as_deref().unwrap_or("doing");
    let default = format!("reopen {} to {}", task.task_key, target);
    let message = read_commit_message(stdin, &default)?;
    let updated = app
        .lun
        .reopen_task(task.id, Some(target), Some(&message), None)?;
    Ok(format!(
        "Reopened {} ({})\nCommitted: {}",
        updated.task_key, updated.title, message
    ))
}

pub fn task_archive(app: &App, query: &str, stdin: &mut dyn BufRead) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let default = format!("archive {}", task.task_key);
    let message = read_commit_message(stdin, &default)?;
    let updated = app.lun.archive_task(task.id, Some(&message), None)?;
    Ok(format!(
        "Archived {} ({})\nCommitted: {}",
        updated.task_key, updated.title, message
    ))
}

pub fn task_set_status(
    app: &App,
    query: &str,
    status: &str,
    message: Option<String>,
    stdin: &mut dyn BufRead,
) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let status = resolve_task_status(status).ok_or_else(|| {
        DbError::new(
            "invalid",
            format!(
                "invalid status '{status}' (expected todo, doing, follow-up, blocked, or done)"
            ),
        )
    })?;
    let default_message = format!("set {} status to {}", task.task_key, status);
    let message = read_commit_message_with_optional_override(stdin, message, &default_message)?;
    let updated = app.lun.update_task(
        task.id,
        TaskUpdateSpec {
            status: Some(status.clone()),
            message: Some(message.clone()),
            ..Default::default()
        },
    )?;
    Ok(format!(
        "Updated {}: status -> {}\nCommitted: {}",
        updated.task_key, status, message
    ))
}

pub fn create_project_command(
    app: &App,
    name: &str,
    status: Option<&str>,
    message: Option<String>,
    stdin: &mut dyn BufRead,
) -> Result<String> {
    if name.trim().is_empty() {
        return Err(DbError::new("usage", "project name must not be empty"));
    }
    let resolved_status = status
        .map(|s| {
            resolve_project_status(s).ok_or_else(|| {
                DbError::new(
                    "invalid",
                    format!("invalid project status '{s}' (expected active or inactive)"),
                )
            })
        })
        .transpose()?
        .unwrap_or_else(|| "active".to_string());
    let default_message = format!("create project \"{}\"", name.trim());
    let commit = read_commit_message_with_optional_override(stdin, message, &default_message)?;
    let project = app.lun.create_project(crate::db::ProjectSpec {
        name: name.trim().to_string(),
        status: Some(resolved_status),
        message: Some(commit.clone()),
        user: None,
    })?;
    Ok(format!(
        "Created project {} [{}]\nCommitted: {}",
        project.name, project.project_key, commit
    ))
}

pub fn project_set_status(
    app: &App,
    query: &str,
    status: &str,
    message: Option<String>,
    stdin: &mut dyn BufRead,
) -> Result<String> {
    let project = resolve_project(&app.lun, query)?;
    let resolved_status = resolve_project_status(status).ok_or_else(|| {
        DbError::new(
            "invalid",
            format!("invalid project status '{status}' (expected active or inactive)"),
        )
    })?;
    let default_message = format!("set {} status to {}", project.project_key, resolved_status);
    let commit = read_commit_message_with_optional_override(stdin, message, &default_message)?;
    let updated =
        app.lun
            .update_project_status(project.id, &resolved_status, Some(&commit), None)?;
    Ok(format!(
        "Updated project {} [{}]: status -> {}\nCommitted: {}",
        updated.name, updated.project_key, updated.status, commit
    ))
}

pub fn move_task(
    app: &App,
    task_query: &str,
    project_query: &str,
    message: Option<String>,
    stdin: &mut dyn BufRead,
) -> Result<String> {
    let task = resolve_task(&app.lun, task_query)?;
    let source_project = app.lun.project_name_for_task(&task);
    let target_project = resolve_project(&app.lun, project_query)?;
    if task.project_id == Some(target_project.id) {
        return Ok(format!(
            "{} already in {}",
            task.task_key, target_project.name
        ));
    }
    let default_message = format!(
        "Moved task from \"{}\" to \"{}\"",
        source_project, target_project.name
    );
    let commit = read_commit_message_with_optional_override(stdin, message, &default_message)?;
    app.lun.update_task(
        task.id,
        TaskUpdateSpec {
            project_id: Some(target_project.id),
            message: Some(commit.clone()),
            ..Default::default()
        },
    )?;
    Ok(format!(
        "Moved {} from {} to {}\nCommitted: {}",
        task.task_key, source_project, target_project.name, commit
    ))
}

// ---------------------------------------------------------------------------
// Interactive task creation
// ---------------------------------------------------------------------------

fn read_prompt(stdin: &mut dyn BufRead, label: &str) -> Result<String> {
    eprint!("{label}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    stdin
        .read_line(&mut line)
        .map_err(|e| DbError::new("io", format!("reading prompt input: {e}")))?;
    Ok(line.trim().to_string())
}

fn valid_task_status(s: &str) -> bool {
    matches!(s, "todo" | "doing" | "follow-up" | "blocked" | "done")
}

fn valid_project_status(s: &str) -> bool {
    matches!(s, "active" | "inactive")
}

fn valid_priority(s: &str) -> bool {
    matches!(s, "low" | "med" | "high")
}

fn resolve_task_status(value: &str) -> Option<String> {
    match value {
        "in-progress" => Some("doing".to_string()),
        "review" => Some("follow-up".to_string()),
        other if valid_task_status(other) => Some(other.to_string()),
        _ => None,
    }
}

fn resolve_project_status(value: &str) -> Option<String> {
    match value {
        "planning" | "in-progress" => Some("active".to_string()),
        "done" => Some("inactive".to_string()),
        other if valid_project_status(other) => Some(other.to_string()),
        _ => None,
    }
}

fn read_commit_message(stdin: &mut dyn BufRead, default: &str) -> Result<String> {
    let ans = read_prompt(stdin, "Commit Message: ")?;
    if ans.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(ans)
    }
}

fn read_commit_message_with_optional_override(
    stdin: &mut dyn BufRead,
    explicit: Option<String>,
    default: &str,
) -> Result<String> {
    if let Some(message) = explicit {
        Ok(message)
    } else {
        read_commit_message(stdin, default)
    }
}

/// `lun add task "<title>" [proj "<name|P-00N>"]`.
///
/// Prompts (printed to stderr; empty input accepts the default):
/// - `Status? (todo, doing, follow-up, blocked, done):` (default: todo)
/// - `Priority? (low, med, or high):` (default: low)
/// - `Assignee? (default: me):`
/// - `Commit Message:` — default `add task "<title>" to <project>`.
///
/// Returns the two-line user output:
/// `Created task T-00N in project X` / `Committed: <msg>`.
pub fn create_task(
    app: &App,
    title: &str,
    project_query: Option<&str>,
    stdin: &mut dyn BufRead,
) -> Result<String> {
    if title.trim().is_empty() {
        return Err(DbError::new(
            "usage",
            "task title must not be empty (quote titles with whitespace)",
        ));
    }
    let project = if let Some(query) = project_query {
        resolve_project(&app.lun, query)?
    } else {
        app.lun.project_by_key("P-000")?
    };

    let status = read_prompt(stdin, "Status? (todo, doing, follow-up, blocked, done): ")?;
    let status = if status.is_empty() {
        "todo".to_string()
    } else {
        resolve_task_status(&status).ok_or_else(|| {
            DbError::new(
                "invalid",
                format!(
                    "invalid status '{status}' (expected todo, doing, follow-up, blocked, or done)"
                ),
            )
        })?
    };

    let priority = read_prompt(stdin, "Priority? (low, med, or high): ")?;
    let priority = if priority.is_empty() {
        "low".to_string()
    } else {
        if !valid_priority(&priority) {
            return Err(DbError::new(
                "invalid",
                format!("invalid priority '{priority}' (expected low, med, or high)"),
            ));
        }
        priority
    };

    let assignee = read_prompt(stdin, "Assignee? (default: me): ")?;
    let assignee = if assignee.is_empty() {
        "me".to_string()
    } else {
        assignee
    };

    let default_msg = format!("add task \"{title}\" to {}", project.name);
    let message = read_commit_message(stdin, &default_msg)?;

    let task = app.lun.create_task(TaskSpec {
        title: title.to_string(),
        project: Some(project.id),
        status: Some(status),
        priority: Some(priority),
        assignee: Some(assignee),
        branch: None,
        labels: None,
        message: Some(message.clone()),
        user: None,
    })?;

    Ok(format!(
        "Created task {} in project {}\nCommitted: {message}",
        task.task_key, project.name
    ))
}

// ---------------------------------------------------------------------------
// Phase 9: PRs (GitHub-style workflow + git glue)
// ---------------------------------------------------------------------------

fn is_pr_key(s: &str) -> bool {
    let rest = s.strip_prefix("PR-").unwrap_or("");
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

/// `lun pr new T-008 --from feature/branch --to main`.
///
/// `--from` defaults to the task's recorded `branch` field (the kanban
/// tracks the branch per task, so most PRs need no flag); `--to` defaults
/// to `main`. The DB layer enforces one open PR per task and rejects
/// source == target.
pub fn pr_new(app: &App, task_query: &str, from: Option<&str>, to: Option<&str>) -> Result<String> {
    let t = resolve_task(&app.lun, task_query)?;
    let pr = app.lun.create_pr(PrSpec {
        task_id: t.id,
        source_branch: from.map(|s| s.to_string()),
        target_branch: to.map(|s| s.to_string()),
        message: None,
        user: None,
    })?;
    Ok(format!(
        "Opened {} for {} ({} -> {})\nCommitted: open {} from {} into {} for {}",
        pr.pr_key,
        t.task_key,
        pr.source_branch,
        pr.target_branch,
        pr.pr_key,
        pr.source_branch,
        pr.target_branch,
        t.task_key
    ))
}

/// Resolve a `lun pr show`/`merge` argument: a `PR-00N` key, or a task
/// key/title (which must have exactly one open PR — more than one is
/// ambiguous, zero is a clean error).
pub fn resolve_pr(app: &App, query: &str) -> Result<Pr> {
    if is_pr_key(query) {
        return app.lun.pr_by_key(query);
    }
    let t = resolve_task(&app.lun, query)?;
    let open = app
        .lun
        .prs_for_task(t.id)?
        .into_iter()
        .filter(|p| p.status == "open")
        .collect::<Vec<_>>();
    match open.as_slice() {
        [pr] => Ok(pr.clone()),
        [] => Err(DbError::new(
            "not-found",
            format!(
                "{} has no open PR (open one with `lun pr new {} --from <branch> --to main`)",
                t.task_key, t.task_key
            ),
        )),
        many => Err(DbError::new(
            "ambiguous",
            format!(
                "{} has {} open PRs: {} — use the PR key",
                t.task_key,
                many.len(),
                many.iter()
                    .map(|p| p.pr_key.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

/// `lun pr show PR-001` (or a task key/title with one open PR).
pub fn pr_show(app: &App, query: &str) -> Result<String> {
    let pr = resolve_pr(app, query)?;
    let task = app.lun.task_by_id(pr.task_id)?;
    let header = format!("PR {}", pr.pr_key);
    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    out.push_str(&"=".repeat(header.chars().count()));
    out.push_str("\n\n");
    out.push_str(&format!(
        "Task:      {} \"{}\"\n",
        task.task_key, task.title
    ));
    out.push_str(&format!("Source:    {}\n", pr.source_branch));
    out.push_str(&format!("Target:    {}\n", pr.target_branch));
    out.push_str(&format!("Status:    {}\n", pr.status));
    out.push_str(&format!("Opened:    {}\n", display_ts(&pr.created_at)));
    if let Some(merged) = &pr.merged_at {
        out.push_str(&format!("Merged:    {}\n", display_ts(merged)));
    }
    out.push('\n');
    // The PR's lifecycle in the task's log: the entries whose details
    // carry this PR's key (the CREATE/UPDATE on open, the MERGE on merge).
    out.push_str("History (log):\n");
    let entries = app.lun.logs_for("task", pr.task_id)?;
    let mine: Vec<_> = entries
        .iter()
        .filter(|e| json_str(&e.details, "pr").as_deref() == Some(pr.pr_key.as_str()))
        .collect();
    if mine.is_empty() {
        out.push_str("- (no PR log entries)");
    } else {
        for e in &mine {
            for line in task_view_entry_lines(e) {
                out.push_str(&format!("{line}\n"));
            }
        }
    }
    Ok(out)
}

/// `lun pr ls` — open PRs first (the working set), then merged, newest
/// first within each group.
pub fn pr_ls(app: &App) -> Result<String> {
    let prs = app.lun.list_prs()?;
    let tasks = app.lun.list_tasks()?;
    let task_key = |id: i64| {
        tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.task_key.clone())
            .unwrap_or_else(|| format!("<task {}>", id))
    };
    // Newest first within each group (rowid order is creation order).
    let open: Vec<&Pr> = prs.iter().filter(|p| p.status == "open").rev().collect();
    let merged: Vec<&Pr> = prs.iter().filter(|p| p.status == "merged").rev().collect();

    let mut out = String::new();
    if open.is_empty() && merged.is_empty() {
        out.push_str(
            "No PRs yet (open one with `lun pr new <T-00N|title> --from <branch> --to main`).",
        );
        return Ok(out);
    }
    if !open.is_empty() {
        out.push_str("Open PRs\n--------\n\n");
        for pr in &open {
            out.push_str(&format!(
                "{}  {}  {} -> {}  opened {}\n",
                pr.pr_key,
                task_key(pr.task_id),
                pr.source_branch,
                pr.target_branch,
                display_ts(&pr.created_at)
            ));
        }
    }
    if !merged.is_empty() {
        if !open.is_empty() {
            out.push('\n');
        }
        out.push_str("Merged PRs\n----------\n\n");
        for pr in &merged {
            let merged_ts = pr
                .merged_at
                .as_deref()
                .map(display_ts)
                .unwrap_or_else(|| "?".to_string());
            out.push_str(&format!(
                "{}  {}  {} -> {}  merged {}\n",
                pr.pr_key,
                task_key(pr.task_id),
                pr.source_branch,
                pr.target_branch,
                merged_ts
            ));
        }
    }
    Ok(out.trim_end().to_string())
}

/// `lun pr merge PR-001` (or a task key/title with one open PR).
///
/// The logical merge (DB row -> `merged`, task -> `done`, `MERGE` log
/// entry) always happens first; the optional `git merge` glue runs
/// after, and a git failure is reported but does NOT undo the logical
/// merge (lun's log is the canonical record — the plan keeps git
/// integration explicitly optional).
pub fn pr_merge(app: &App, query: &str, root: &Path) -> Result<String> {
    let pr = resolve_pr(app, query)?;
    let merged = app.lun.merge_pr(pr.id, None, None)?;
    let mut out = format!(
        "Merged {} ({} -> {}); task {} is now done\nCommitted: merge {} ({} -> {})",
        merged.pr_key,
        merged.source_branch,
        merged.target_branch,
        app.lun.task_by_id(merged.task_id)?.task_key,
        merged.pr_key,
        merged.source_branch,
        merged.target_branch
    );

    // Optional git glue: only when `root` is a git repo and both
    // branches exist locally. Never fails the command — reports instead.
    if let Some(note) = git_merge_note(root, &merged.source_branch, &merged.target_branch) {
        out.push('\n');
        out.push_str(&note);
    }
    Ok(out)
}

/// Run the optional `git merge` glue for a merged PR. Returns a report
/// line, or `None` when glue doesn't apply (not a git repo / missing
/// branch).
fn git_merge_note(root: &Path, source: &str, target: &str) -> Option<String> {
    let is_repo = std::process::Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .output()
        .ok()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false);
    if !is_repo {
        return None;
    }
    // Does the source branch exist locally?
    let has_source = std::process::Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-parse",
            "--verify",
            "--quiet",
            source,
        ])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has_source {
        return Some(format!(
            "(git: branch '{source}' not found in this repo — logical merge recorded only)"
        ));
    }
    let on_target = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "branch", "--show-current"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if on_target != target {
        return Some(format!(
            "(git: currently on '{on_target}', not '{target}' — run `git checkout {target} && git merge {source}` to finish)"
        ));
    }
    let merge = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "merge", "--no-edit", source])
        .output();
    match merge {
        Ok(o) if o.status.success() => Some(format!(
            "git: merged {source} into {target} (--no-edit; commit message from git)"
        )),
        Ok(o) => Some(format!(
            "(git merge failed: {} — logical merge still recorded; resolve with `git merge {source}`)",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Some(format!("(git merge could not run: {e} — logical merge still recorded)")),
    }
}

fn parse_message_flag(args: &[String]) -> Result<(Vec<String>, Option<String>)> {
    let mut out = Vec::new();
    let mut message: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--message" {
            let value = args
                .get(i + 1)
                .ok_or_else(|| DbError::new("usage", "--message needs a value"))?;
            message = Some(value.clone());
            i += 2;
        } else {
            out.push(args[i].clone());
            i += 1;
        }
    }
    Ok((out, message))
}

// ---------------------------------------------------------------------------
// Dispatch (used by the binary)
// ---------------------------------------------------------------------------

/// `lun pr` subcommand dispatch: `new` (with `--from`/`--to` flags),
/// `show`, `ls`, `merge`.
fn run_pr(app: &App, args: &[String]) -> Result<String> {
    let Some(sub) = args.first() else {
        return Err(DbError::new(
            "usage",
            "expected: lun pr <new|show|ls|merge> (see `lun --help`)",
        ));
    };
    match sub.as_str() {
        "ls" => {
            if args.len() > 1 {
                return Err(DbError::new("usage", "`lun pr ls` takes no arguments"));
            }
            pr_ls(app)
        }
        "new" => {
            let mut task: Option<&str> = None;
            let mut from: Option<String> = None;
            let mut to: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--from" => {
                        from = Some(
                            args.get(i + 1)
                                .ok_or_else(|| DbError::new("usage", "`--from` needs <branch>"))?
                                .clone(),
                        );
                        i += 2;
                    }
                    "--to" => {
                        to = Some(
                            args.get(i + 1)
                                .ok_or_else(|| DbError::new("usage", "`--to` needs <branch>"))?
                                .clone(),
                        );
                        i += 2;
                    }
                    other if task.is_none() && !other.starts_with("--") => {
                        task = Some(other);
                        i += 1;
                    }
                    other => {
                        return Err(DbError::new(
                            "usage",
                            format!("unexpected argument '{other}'"),
                        ));
                    }
                }
            }
            let Some(task) = task else {
                return Err(DbError::new(
                    "usage",
                    "expected: lun pr new <T-00N|title> [--from <branch>] [--to <branch>]",
                ));
            };
            pr_new(app, task, from.as_deref(), to.as_deref())
        }
        "show" | "merge" => {
            if args.len() != 2 {
                return Err(DbError::new(
                    "usage",
                    format!("expected: lun pr {sub} <PR-00N|T-00N|title>"),
                ));
            }
            match sub.as_str() {
                "show" => pr_show(app, &args[1]),
                "merge" => match std::env::current_dir() {
                    Ok(root) => pr_merge(app, &args[1], &root),
                    Err(e) => Err(DbError::new("io", format!("resolving CWD: {e}"))),
                },
                _ => unreachable!(),
            }
        }
        _ => Err(DbError::new(
            "usage",
            format!("unknown subcommand 'lun pr {sub}' (expected new, show, ls, or merge)"),
        )),
    }
}

pub fn run_result_in_reader(
    app: &App,
    args: &[String],
    stdin: &mut dyn BufRead,
    root_override: Option<&Path>,
) -> Result<String> {
    match args.first().map(String::as_str) {
        Some("complete") => Ok(complete_output(Some(app), &args[1..])),
        Some("init") => init_db_command(app),
        Some("help") | Some("--help") => Ok(help_text(env!("CARGO_PKG_VERSION"))),
        Some("--version") => Ok(format!("lun v{}", env!("CARGO_PKG_VERSION"))),
        Some("status") => match args.get(1) {
            None => status_all(app),
            Some(q) => {
                let board = args.iter().skip(2).any(|a| a == "--board");
                if board {
                    status_project_board(app, q)
                } else {
                    status_target(app, q)
                }
            }
        },
        Some("new") => match args.get(1).map(String::as_str) {
            Some("proj") | Some("project") => {
                let name = args
                    .get(2)
                    .ok_or_else(|| DbError::new("usage", "expected: lun new proj \"<name>\""))?;
                let (filtered, message) = parse_message_flag(&args[3..])?;
                let mut status: Option<&str> = None;
                let mut i = 0;
                while i < filtered.len() {
                    match filtered[i].as_str() {
                        "--status" => {
                            status = Some(
                                filtered
                                    .get(i + 1)
                                    .ok_or_else(|| DbError::new("usage", "--status needs a value"))?,
                            );
                            i += 2;
                        }
                        other => {
                            return Err(DbError::new(
                                "usage",
                                format!("unexpected argument '{other}' for lun new proj"),
                            ))
                        }
                    }
                }
                create_project_command(app, name, status, message, stdin)
            }
            _ => Err(DbError::new(
                "usage",
                "expected: lun new proj \"<name>\" [--status active|inactive] [--message \"...\"]",
            )),
        },
        Some("add") => match args.get(1).map(String::as_str) {
            Some("task") => {
                let title = args
                    .get(2)
                    .ok_or_else(|| DbError::new("usage", "expected: lun add task \"<title>\" [proj \"<project>\"]"))?;
                let mut project_query: Option<&str> = None;
                let mut i = 3;
                while i < args.len() {
                    match args[i].as_str() {
                        "proj" | "project" => {
                            project_query = Some(
                                args.get(i + 1)
                                    .ok_or_else(|| DbError::new("usage", "proj needs a project name or key"))?,
                            );
                            i += 2;
                        }
                        other => {
                            return Err(DbError::new(
                                "usage",
                                format!(
                                    "unexpected argument '{other}' (expected: lun add task \"<title>\" [proj \"<project>\"])"
                                ),
                            ))
                        }
                    }
                }
                create_task(app, title, project_query, stdin)
            }
            _ => Err(DbError::new(
                "usage",
                "expected: lun add task \"<title>\" [proj \"<project>\"]",
            )),
        },
        Some("proj") => match (args.get(1), args.get(2), args.get(3)) {
            (Some(a), Some(b), Some(title)) if a == "add" && b == "task" => {
                let projects = app.lun.list_projects()?;
                let user_projects: Vec<_> = projects
                    .iter()
                    .filter(|p| p.project_key != "P-000")
                    .collect();
                let project_query = if user_projects.len() == 1 {
                    Some(user_projects[0].name.as_str())
                } else {
                    None
                };
                create_task(app, title, project_query, stdin)
            }
            _ => {
                let query = args
                    .get(1)
                    .ok_or_else(|| DbError::new("usage", "expected: lun proj <name|P-00N> --status <active|inactive>"))?;
                let (filtered, message) = parse_message_flag(&args[2..])?;
                let mut status: Option<&str> = None;
                let mut i = 0;
                while i < filtered.len() {
                    match filtered[i].as_str() {
                        "--status" => {
                            status = Some(
                                filtered
                                    .get(i + 1)
                                    .ok_or_else(|| DbError::new("usage", "--status needs a value"))?,
                            );
                            i += 2;
                        }
                        other => {
                            return Err(DbError::new(
                                "usage",
                                format!("unexpected argument '{other}' for lun proj"),
                            ))
                        }
                    }
                }
                let status = status.ok_or_else(|| {
                    DbError::new(
                        "usage",
                        "expected: lun proj <name|P-00N> --status <active|inactive> [--message \"...\"]",
                    )
                })?;
                project_set_status(app, query, status, message, stdin)
            }
        },
        Some("move") => {
            let task_query = args
                .get(1)
                .ok_or_else(|| DbError::new("usage", "expected: lun move \"<task>\" \"<project>\""))?;
            let project_query = args
                .get(2)
                .ok_or_else(|| DbError::new("usage", "expected: lun move \"<task>\" \"<project>\""))?;
            let (_, message) = parse_message_flag(&args[3..])?;
            move_task(app, task_query, project_query, message, stdin)
        }
        Some("task") => match args.get(1) {
            Some(sub) if sub == "ls" => task_list(app, &args[2..]),
            Some(sub) if sub == "edit" => match args.get(2) {
                Some(q) => task_edit(app, q, &args[3..], stdin),
                None => Err(DbError::new("usage", "expected: lun task edit <key|title> [flags]")),
            },
            Some(sub) if sub == "complete" => match args.get(2) {
                Some(q) => task_complete(app, q, stdin),
                None => Err(DbError::new("usage", "expected: lun task complete <key|title>")),
            },
            Some(sub) if sub == "reopen" => match args.get(2) {
                Some(q) => task_reopen(app, q, &args[3..], stdin),
                None => Err(DbError::new("usage", "expected: lun task reopen <key|title>")),
            },
            Some(sub) if sub == "archive" || sub == "delete" => match args.get(2) {
                Some(q) => task_archive(app, q, stdin),
                None => Err(DbError::new("usage", "expected: lun task archive <key|title>")),
            },
            Some(q) => {
                if args.len() == 2 {
                    task_view(app, q)
                } else {
                    let (filtered, message) = parse_message_flag(&args[2..])?;
                    let mut status: Option<&str> = None;
                    let mut i = 0;
                    while i < filtered.len() {
                        match filtered[i].as_str() {
                            "--status" => {
                                status = Some(
                                    filtered
                                        .get(i + 1)
                                        .ok_or_else(|| DbError::new("usage", "--status needs a value"))?,
                                );
                                i += 2;
                            }
                            other => {
                                return Err(DbError::new(
                                    "usage",
                                    format!("unexpected argument '{other}' for lun task"),
                                ))
                            }
                        }
                    }
                    let status = status.ok_or_else(|| {
                        DbError::new(
                            "usage",
                            "expected: lun task <key|title> [--status <todo|doing|follow-up|blocked|done>] [--message \"...\"]",
                        )
                    })?;
                    task_set_status(app, q, status, message, stdin)
                }
            }
            None => Err(DbError::new(
                "usage",
                "expected: lun task <key|title> [--status ...] | task ls | task edit | task complete | task reopen | task archive",
            )),
        },
        Some("log") => match args.get(1) {
            Some(q) => log_view(app, q),
            None => Err(DbError::new("usage", "expected: lun log <project|task>")),
        },
        Some("attach") => match args.get(1).map(String::as_str) {
            Some("ls") => match (args.get(2), args.get(3)) {
                (Some(kind), Some(q)) => list_attachments(app, kind, q),
                _ => Err(DbError::new(
                    "usage",
                    "expected: lun attach ls <task|project> <key|title>",
                )),
            },
            Some("rm") => match (args.get(2), args.get(3), args.get(4)) {
                (Some(kind), Some(q), Some(needle)) => remove_attachment_command(app, kind, q, needle),
                _ => Err(DbError::new(
                    "usage",
                    "expected: lun attach rm <task|project> <key|title> <filename|id>",
                )),
            },
            Some("open") => match (args.get(2), args.get(3), args.get(4)) {
                (Some(kind), Some(q), Some(needle)) => open_attachment_command(app, kind, q, needle),
                _ => Err(DbError::new(
                    "usage",
                    "expected: lun attach open <task|project> <key|title> <filename|id>",
                )),
            },
            Some(kind) if kind == "task" || kind == "project" => match (args.get(1), args.get(2), args.get(3)) {
                (Some(kind), Some(q), Some(file)) => {
                let root = match root_override {
                    Some(root) => Ok(root.to_path_buf()),
                    None => std::env::current_dir()
                        .map_err(|e| DbError::new("io", format!("resolving CWD: {e}"))),
                }?;
                attach_entity_file(app, &root, kind, q, file, stdin)
            }
                _ => Err(DbError::new(
                    "usage",
                    "expected: lun attach <task|project> <key|title> /path/to/file",
                )),
            },
            _ => Err(DbError::new(
                "usage",
                "expected: lun attach <task|project> <key|title> /path/to/file | attach ls | attach rm | attach open",
            )),
        },
        Some("link") => match (args.get(1), args.get(2), args.get(3), args.get(4)) {
            (Some(kind), Some(q), Some(label), Some(uri)) => {
                add_link_command(app, kind, q, label, uri)
            }
            _ => Err(DbError::new(
                "usage",
                "expected: lun link <task|project> <key|title> \"<label>\" \"<uri>\"",
            )),
        },
        Some("open-link") => match (args.get(1), args.get(2), args.get(3)) {
            (Some(kind), Some(q), Some(label)) => open_link(app, kind, q, label),
            _ => Err(DbError::new(
                "usage",
                "expected: lun open-link <task|project> <key|title> <label>",
            )),
        },
        Some("open-uri") => open_uri(app, &args[1..]),
        Some("pr") => match args.get(1).map(String::as_str) {
            Some("merge") if root_override.is_some() && args.len() == 3 => {
                pr_merge(app, &args[2], root_override.unwrap())
            }
            _ => run_pr(app, &args[1..]),
        },
        Some(other) => Err(DbError::new(
            "usage",
            format!("command '{other}' not implemented (see `lun --help`)"),
        )),
        None => Err(DbError::new("usage", "no command given (see `lun --help`)")),
    }
}

pub fn run_result_with_reader(
    app: &App,
    args: &[String],
    stdin: &mut dyn BufRead,
) -> Result<String> {
    run_result_in_reader(app, args, stdin, None)
}

fn run_result(app: &App, args: &[String]) -> Result<String> {
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    run_result_with_reader(app, args, &mut reader)
}

/// Dispatch already-split argv (without the program name) to a command.
/// Prints the result to stdout, errors to stderr, and returns the exit code.
/// (2 = usage/resolution error, 1 = runtime (DB/IO) error.)
pub fn run(app: &App, args: &[String]) -> ExitCode {
    let result = run_result(app, args);

    match result {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            let code = match e.kind() {
                "usage" | "not-found" | "ambiguous" | "invalid" | "declined" => EXIT_USAGE,
                _ => EXIT_RUNTIME,
            };
            eprintln!("lun: {e}");
            ExitCode::from(code)
        }
    }
}
