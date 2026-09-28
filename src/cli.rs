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

use crate::db::Result;
use crate::db::{
    Attachment, AttachmentTarget, DbError, Link, LinkTarget, LogEntry, Lun, Pr, PrSpec, Project,
    Task, TaskListSpec, TaskSort, TaskSpec, TaskUpdateSpec,
};

/// CLI exit codes: 2 = usage/resolution error, 1 = runtime (DB/IO) error.
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_RUNTIME: u8 = 1;

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

/// Per-project task counts: Open = todo + in-progress, Review, Done.
fn status_counts(tasks: &[Task]) -> (usize, usize, usize) {
    let open = tasks
        .iter()
        .filter(|t| t.status == "todo" || t.status == "in-progress")
        .count();
    let review = tasks.iter().filter(|t| t.status == "review").count();
    let done = tasks.iter().filter(|t| t.status == "done").count();
    (open, review, done)
}

fn counts_by_status(tasks: &[Task]) -> Vec<(&'static str, usize)> {
    let mut out = Vec::new();
    for s in ["todo", "in-progress", "review", "done"] {
        out.push((s, tasks.iter().filter(|t| t.status == s).count()));
    }
    out
}

/// The plan's summary line: `Summary: N projects · M tasks (a todo, b in-progress, c review, d done)`.
pub fn summary_line(n_projects: usize, tasks: &[Task]) -> String {
    let (t, ip, r, d) = (
        tasks.iter().filter(|t| t.status == "todo").count(),
        tasks.iter().filter(|t| t.status == "in-progress").count(),
        tasks.iter().filter(|t| t.status == "review").count(),
        tasks.iter().filter(|t| t.status == "done").count(),
    );
    format!(
        "Summary: {n_projects} project{} · {} task{} ({t} todo, {ip} in-progress, {r} review, {d} done)",
        if n_projects == 1 { "" } else { "s" },
        tasks.len(),
        if tasks.len() == 1 { "" } else { "s" }
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

/// `lun status` — global project table, all-task table, summary.
pub fn status_all(app: &App) -> Result<String> {
    let projects = app.lun.list_projects()?;
    let tasks = app.lun.list_tasks()?;

    let mut prow = Vec::new();
    for p in &projects {
        let (open, review, done) = status_counts(&app.lun.tasks_for_project(p.id)?);
        prow.push(vec![
            p.project_key.clone(),
            p.name.clone(),
            p.status.clone(),
            open.to_string(),
            review.to_string(),
            done.to_string(),
        ]);
    }

    let trow: Vec<Vec<String>> = tasks.iter().map(|t| task_row(&app.lun, t, true)).collect();

    let mut out = String::new();
    out.push_str("Projects\n--------\n\n");
    out.push_str(&render_table(
        &["ID", "Name", "Status", "Open", "Review", "Done"],
        &prow,
    ));
    out.push_str("\n\n\nTasks\n-----\n\n");
    out.push_str(&render_table(
        &[
            "ID", "Project", "Title", "Status", "Priority", "Assignee", "Branch",
        ],
        &trow,
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
    let trow: Vec<Vec<String>> = tasks.iter().map(|t| task_row(&app.lun, t, false)).collect();
    out.push_str(&render_table(
        &["ID", "Title", "Status", "Priority", "Assignee", "Branch"],
        &trow,
    ));
    out.push('\n');
    out.push_str(&summary_line(1, &tasks));
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
    let (header, is_project) = match &entity {
        Entity::Project(p) => (format!("Log: {}", p.name), true),
        Entity::Task(t) => (format!("Log: Task {} \"{}\"", t.task_key, t.title), false),
    };
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
    out.push_str(&header);
    out.push('\n');
    out.push_str(&"=".repeat(header.chars().count()));
    out.push('\n');
    for e in &entries {
        out.push('\n');
        let lines = if is_project {
            project_log_entry_lines(e)
        } else {
            task_log_entry_lines(e)
        };
        for line in lines {
            out.push_str(&format!("{line}\n"));
        }
    }
    Ok(out)
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

pub fn task_edit(app: &App, query: &str, args: &[String]) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let mut spec = TaskUpdateSpec::default();
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
                spec.message = Some(value(i, "--message", args)?);
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
    let updated = app.lun.update_task(task.id, spec)?;
    Ok(format!(
        "Updated {}: {} [{} / {}]",
        updated.task_key, updated.title, updated.status, updated.priority
    ))
}

pub fn task_complete(app: &App, query: &str) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let updated = app.lun.complete_task(task.id, None, None)?;
    Ok(format!(
        "Completed {} ({})",
        updated.task_key, updated.title
    ))
}

pub fn task_reopen(app: &App, query: &str, args: &[String]) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let mut status: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--status" => {
                status = Some(
                    args.get(i + 1)
                        .ok_or_else(|| DbError::new("usage", "--status needs a value"))?
                        .as_str(),
                );
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
    let updated = app.lun.reopen_task(task.id, status, None, None)?;
    Ok(format!("Reopened {} ({})", updated.task_key, updated.title))
}

pub fn task_archive(app: &App, query: &str) -> Result<String> {
    let task = resolve_task(&app.lun, query)?;
    let updated = app.lun.archive_task(task.id, None, None)?;
    Ok(format!("Archived {} ({})", updated.task_key, updated.title))
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
    matches!(s, "todo" | "in-progress" | "review" | "done")
}

fn valid_priority(s: &str) -> bool {
    matches!(s, "low" | "med" | "high")
}

/// `lun proj add task "<title>"`.
///
/// Prompts (printed to stderr; empty input accepts the default):
/// - `Status?:` (default: todo; must be todo|in-progress|review|done)
/// - `Priority?:` (default: low; must be low|med|high)
/// - `Assignee? (default: me):`
/// - `Project?:` — default is the sole project when exactly one exists,
///   otherwise Unassigned (P-000); accepts a project name or key.
/// - `Commit message?:` — default `add task "<title>" to <project>` (the
///   same default the DB layer would use).
///
/// Returns the two-line user output:
/// `Created task T-00N in project X` / `Committed: <msg>`.
pub fn create_task(app: &App, title: &str, stdin: &mut dyn BufRead) -> Result<String> {
    if title.trim().is_empty() {
        return Err(DbError::new(
            "usage",
            "task title must not be empty (quote titles with whitespace)",
        ));
    }
    let projects = app.lun.list_projects()?;
    // Default project: the sole user project if exactly one exists, otherwise
    // the implicit P-000 Unassigned bucket (P-000 never counts as a user pick).
    let user_projects: Vec<_> = projects
        .iter()
        .filter(|p| p.project_key != "P-000")
        .cloned()
        .collect();
    let default_project = if user_projects.len() == 1 {
        user_projects.into_iter().next().unwrap()
    } else {
        app.lun.project_by_key("P-000")?
    };

    let status = read_prompt(stdin, "Status?: ")?;
    let status = if status.is_empty() {
        "todo".to_string()
    } else {
        if !valid_task_status(&status) {
            return Err(DbError::new(
                "invalid",
                format!("invalid status '{status}' (expected todo, in-progress, review, or done)"),
            ));
        }
        status
    };

    let priority = read_prompt(stdin, "Priority?: ")?;
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

    // Project: the sole user project when exactly one exists, otherwise the
    // implicit P-000 Unassigned bucket (the plan's flow has no project prompt).
    let project = default_project;

    let default_msg = format!("add task \"{title}\" to {}", project.name);
    let msg_ans = read_prompt(stdin, &format!("Commit message?: "))?;
    let message = if msg_ans.is_empty() {
        default_msg
    } else {
        msg_ans
    };

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

/// Dispatch already-split argv (without the program name) to a command.
/// Prints the result to stdout, errors to stderr, and returns the exit code.
/// (2 = usage/resolution error, 1 = runtime (DB/IO) error.)
pub fn run(app: &App, args: &[String]) -> ExitCode {
    let result: Result<String> = match args.first().map(String::as_str) {
        Some("status") => match args.get(1) {
            None => status_all(app),
            Some(q) => status_project(app, q),
        },
        Some("proj") => match (args.get(1), args.get(2), args.get(3)) {
            (Some(a), Some(b), Some(title)) if a == "add" && b == "task" => {
                let stdin = std::io::stdin();
                let mut reader = std::io::BufReader::new(stdin.lock());
                create_task(app, title, &mut reader)
            }
            _ => Err(DbError::new(
                "usage",
                "expected: lun proj add task \"<title>\"",
            )),
        },
        Some("task") => match args.get(1) {
            Some(sub) if sub == "ls" => task_list(app, &args[2..]),
            Some(sub) if sub == "edit" => match args.get(2) {
                Some(q) => task_edit(app, q, &args[3..]),
                None => Err(DbError::new("usage", "expected: lun task edit <key|title> [flags]")),
            },
            Some(sub) if sub == "complete" => match args.get(2) {
                Some(q) => task_complete(app, q),
                None => Err(DbError::new("usage", "expected: lun task complete <key|title>")),
            },
            Some(sub) if sub == "reopen" => match args.get(2) {
                Some(q) => task_reopen(app, q, &args[3..]),
                None => Err(DbError::new("usage", "expected: lun task reopen <key|title>")),
            },
            Some(sub) if sub == "archive" || sub == "delete" => match args.get(2) {
                Some(q) => task_archive(app, q),
                None => Err(DbError::new("usage", "expected: lun task archive <key|title>")),
            },
            Some(q) => task_view(app, q),
            None => Err(DbError::new(
                "usage",
                "expected: lun task <key|title> | task ls | task edit | task complete | task reopen | task archive",
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
                let stdin = std::io::stdin();
                let mut reader = std::io::BufReader::new(stdin.lock());
                match std::env::current_dir() {
                    Ok(root) => attach_entity_file(app, &root, kind, q, file, &mut reader),
                    Err(e) => Err(DbError::new("io", format!("resolving CWD: {e}"))),
                }
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
        Some("open-uri") => open_uri(app, &args[1..].to_vec()),
        Some("pr") => run_pr(app, &args[1..].to_vec()),
        Some(other) => Err(DbError::new(
            "usage",
            format!("command '{other}' not implemented (see `lun --help`)"),
        )),
        None => Err(DbError::new("usage", "no command given (see `lun --help`)")),
    };

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
