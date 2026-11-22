//! Agent-oriented CLI additions (schema-neutral, purely additive).
//!
//! These exist so an agent can read exactly the slice of the board it needs
//! instead of loading everything into context:
//!
//! - `lun brief`                       one line per open task
//! - `lun task <T> --brief | --log N`  a task without (or with a slice of) its history
//! - `--json` / `--fields a,b`         machine-readable output on `task`, `task ls`, `log`, `brief`
//! - `lun log ... --since <id>`        only entries newer than a log id (cheap polling)
//! - `lun task edit A,B,C ...`         bulk edit / bulk `--status`
//! - `lun add task "<t>" --status ...` flag-driven creation, no stdin prompts
//!
//! This is a child module of `cli`, so it reuses its private helpers.

use super::*;
use crate::db::CLOSED_TASK_STATUS;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Opts {
    pub json: bool,
    pub brief: bool,
    pub oneline: bool,
    pub last: bool,
    pub all: bool,
    pub fields: Option<Vec<String>>,
    pub log: Option<usize>,
    pub since: Option<i64>,
    pub rest: Vec<String>,
}

pub(super) fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut o = Opts::default();
    let mut i = 0;
    let need = |i: usize, flag: &str| -> Result<String> {
        args.get(i + 1)
            .cloned()
            .ok_or_else(|| DbError::new("usage", format!("{flag} needs a value")))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--brief" => o.brief = true,
            "--oneline" => o.oneline = true,
            "--last" => o.last = true,
            "--all" => {
                o.all = true;
                o.rest.push("--all".into());
            }
            "--fields" => {
                let v = need(i, "--fields")?;
                o.fields = Some(
                    v.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect(),
                );
                i += 1;
            }
            "--log" => {
                let v = need(i, "--log")?;
                o.log = Some(v.parse::<usize>().map_err(|_| {
                    DbError::new("usage", format!("--log needs a number, got '{v}'"))
                })?);
                i += 1;
            }
            "--since" => {
                let v = need(i, "--since")?;
                let v = v.trim_start_matches('#');
                o.since = Some(v.parse::<i64>().map_err(|_| {
                    DbError::new("usage", format!("--since needs a log id, got '{v}'"))
                })?);
                i += 1;
            }
            other => o.rest.push(other.to_string()),
        }
        i += 1;
    }
    Ok(o)
}

const TASK_FIELDS: [&str; 14] = [
    "key",
    "title",
    "status",
    "priority",
    "assignee",
    "branch",
    "scope",
    "labels",
    "project",
    "project_key",
    "notes",
    "created_at",
    "updated_at",
    "archived",
];

fn task_map(t: &Task, projects: &[Project]) -> Map<String, Value> {
    let proj = t
        .project_id
        .and_then(|id| projects.iter().find(|p| p.id == id));
    let labels: Vec<String> = serde_json::from_str(&t.labels).unwrap_or_default();
    let mut m = Map::new();
    m.insert("key".into(), json!(t.task_key));
    m.insert("title".into(), json!(t.title));
    m.insert("status".into(), json!(t.status));
    m.insert("priority".into(), json!(t.priority));
    m.insert("assignee".into(), json!(t.assignee));
    m.insert("branch".into(), json!(t.branch));
    m.insert("scope".into(), json!(t.scope));
    m.insert("labels".into(), json!(labels));
    m.insert("project".into(), json!(proj.map(|p| p.name.clone())));
    m.insert("project_key".into(), json!(proj.map(|p| p.project_key.clone())));
    m.insert("notes".into(), json!(t.notes));
    m.insert("created_at".into(), json!(t.created_at));
    m.insert("updated_at".into(), json!(t.updated_at));
    m.insert("archived".into(), json!(t.archived_at.is_some()));
    m
}

/// Keep only `fields` (validated against `valid`), in the requested order.
fn pick(
    mut m: Map<String, Value>,
    fields: &Option<Vec<String>>,
    valid: &[&str],
) -> Result<Map<String, Value>> {
    let Some(fields) = fields else { return Ok(m) };
    let mut out = Map::new();
    for f in fields {
        if !valid.contains(&f.as_str()) {
            return Err(DbError::new(
                "invalid",
                format!("unknown field '{f}' (expected one of: {})", valid.join(", ")),
            ));
        }
        if let Some(v) = m.remove(f) {
            out.insert(f.clone(), v);
        }
    }
    Ok(out)
}

fn scalar(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.split_whitespace().collect::<Vec<_>>().join(" "),
        Value::Array(a) => a.iter().map(scalar).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

fn pretty(v: &Value) -> Result<String> {
    serde_json::to_string_pretty(v)
        .map_err(|e| DbError::new("json", format!("serializing JSON: {e}")))
}

fn entity_keys(app: &App) -> Result<HashMap<(String, i64), String>> {
    let mut map = HashMap::new();
    for p in app.lun.list_projects()? {
        map.insert(("project".to_string(), p.id), p.project_key);
    }
    let spec = TaskListSpec {
        include_archived: true,
        ..Default::default()
    };
    for t in app.lun.list_tasks_with(&spec)? {
        map.insert(("task".to_string(), t.id), t.task_key);
    }
    Ok(map)
}

fn log_json(e: &LogEntry, keys: &HashMap<(String, i64), String>) -> Value {
    let details: Value = serde_json::from_str(&e.details).unwrap_or(json!(e.details));
    json!({
        "id": e.id,
        "entity": keys.get(&(e.entity_type.clone(), e.entity_id)),
        "entity_type": e.entity_type,
        "timestamp": e.timestamp,
        "user": e.user,
        "action": e.action,
        "message": e.message,
        "details": details,
    })
}

const LOG_FIELDS: [&str; 8] = [
    "id",
    "entity",
    "entity_type",
    "timestamp",
    "user",
    "action",
    "message",
    "details",
];

fn status_rank(s: &str) -> usize {
    TASK_STATUSES.iter().position(|x| *x == s).unwrap_or(99)
}

fn priority_rank(p: &str) -> usize {
    match p {
        "high" => 0,
        "med" => 1,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------
// Dispatch hook
// ---------------------------------------------------------------------------

/// Called first from `run_result_in_reader`. `None` = not ours, fall through.
pub(super) fn try_dispatch(
    app: &App,
    args: &[String],
    stdin: &mut dyn BufRead,
) -> Option<Result<String>> {
    let has = |f: &str| args.iter().any(|a| a == f);
    let agent_flag = has("--json") || has("--fields") || has("--brief") || has("--log") || has("--since");
    match args.first().map(String::as_str)? {
        "brief" => Some(brief(app, &args[1..], None)),
        "status" if has("--oneline") && args.get(1).is_some_and(|a| !a.starts_with('-')) => {
            let q = args[1].clone();
            let rest: Vec<String> = args[2..].iter().filter(|a| *a != "--oneline").cloned().collect();
            Some(brief(app, &rest, Some(q)))
        }
        "log" if (has("--json") || has("--since")) && !has("-m") => Some(log_ext(app, &args[1..])),
        "add" if args.get(1).map(String::as_str) == Some("task")
            && ["--status", "--priority", "--assignee", "--scope", "--message", "--branch"]
                .iter()
                .any(|f| has(f)) =>
        {
            Some(add_task_flags(app, &args[2..]))
        }
        "task" => match args.get(1).map(String::as_str)? {
            "ls" if agent_flag || has("--oneline") => Some(task_ls(app, &args[2..])),
            "edit" if args.get(2).is_some_and(|q| q.contains(',')) => {
                Some(bulk_edit(app, &args[2], &args[3..], stdin))
            }
            "ls" | "edit" | "complete" | "reopen" | "archive" | "delete" => None,
            q if q.contains(',') && has("--status") => Some(bulk_status(app, q, &args[2..], stdin)),
            q if !has("--status") && agent_flag => Some(task_view_ext(app, q, &args[2..])),
            _ => None,
        },
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// lun task <T> [--brief] [--log N] [--json] [--fields ...]
// ---------------------------------------------------------------------------

fn task_view_ext(app: &App, query: &str, rest: &[String]) -> Result<String> {
    let o = parse_opts(rest)?;
    if let Some(extra) = o.rest.first() {
        return Err(DbError::new(
            "usage",
            format!("unexpected argument '{extra}' for lun task (expected --brief, --log N, --json, --fields a,b)"),
        ));
    }
    let t = resolve_task(&app.lun, query)?;
    let entries = app.lun.logs_for("task", t.id)?;

    if o.json || o.fields.is_some() {
        let projects = app.lun.list_projects()?;
        let mut m = task_map(&t, &projects);
        let mut valid: Vec<&str> = TASK_FIELDS.to_vec();
        let mut extra: Vec<(&str, Value)> = Vec::new();
        let links: Vec<Value> = app
            .lun
            .links_for_task(t.id)?
            .iter()
            .map(|l| json!({"label": l.label, "uri": l.uri}))
            .collect();
        extra.push(("links", json!(links)));
        extra.push(("history_count", json!(entries.len())));
        if let Some(n) = o.log {
            let keys = entity_keys(app)?;
            let h: Vec<Value> = entries.iter().take(n).map(|e| log_json(e, &keys)).collect();
            extra.push(("history", json!(h)));
        }
        for (k, v) in extra {
            valid.push(k);
            m.insert(k.to_string(), v);
        }
        let m = if o.fields.is_some() {
            pick(m, &o.fields, &valid)?
        } else {
            m
        };
        if o.json {
            return pretty(&Value::Object(m));
        }
        return Ok(m
            .iter()
            .map(|(k, v)| format!("{k}: {}", scalar(v)))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    // Text: reuse the full view, swapping the History section.
    let full = task_view(app, query)?;
    let (hm, pm) = ("\nHistory (log):\n", "\nPRs:\n");
    let (Some(h), Some(p)) = (full.find(hm), full.find(pm)) else {
        return Ok(full);
    };
    if h > p {
        return Ok(full);
    }
    let mid = match (o.brief, o.log) {
        (_, Some(n)) if !o.brief || n > 0 => {
            let mut s = format!("\nHistory (log): newest {} of {}\n", n.min(entries.len()), entries.len());
            for e in entries.iter().take(n) {
                for line in task_view_entry_lines(e) {
                    s.push_str(&format!("{line}\n"));
                }
            }
            s
        }
        _ => format!(
            "\nHistory: {} entries (omitted; `lun log {} -3 --oneline` for the newest)\n",
            entries.len(),
            t.task_key
        ),
    };
    Ok(format!("{}{}{}", &full[..h], mid, &full[p..]))
}

// ---------------------------------------------------------------------------
// lun task ls --json / --fields / --oneline
// ---------------------------------------------------------------------------

fn task_ls(app: &App, rest: &[String]) -> Result<String> {
    let o = parse_opts(rest)?;
    let spec = parse_list_spec(app, &o.rest)?;
    let tasks = app.lun.list_tasks_with(&spec)?;
    let projects = app.lun.list_projects()?;
    if o.oneline && !o.json && o.fields.is_none() {
        return Ok(brief_lines(app, &tasks, &projects, o.last));
    }
    let default: Vec<String> = [
        "key", "title", "status", "priority", "assignee", "branch", "scope", "project_key", "updated_at",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let fields = Some(o.fields.clone().unwrap_or(default));
    let mut rows = Vec::new();
    for t in &tasks {
        rows.push(pick(task_map(t, &projects), &fields, &TASK_FIELDS)?);
    }
    if o.json {
        return pretty(&Value::Array(rows.into_iter().map(Value::Object).collect()));
    }
    if rows.is_empty() {
        return Ok("No tasks.".to_string());
    }
    Ok(rows
        .iter()
        .map(|m| m.values().map(scalar).collect::<Vec<_>>().join("  "))
        .collect::<Vec<_>>()
        .join("\n"))
}

// ---------------------------------------------------------------------------
// lun brief
// ---------------------------------------------------------------------------

fn trunc(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

fn brief_lines(app: &App, tasks: &[Task], projects: &[Project], last: bool) -> String {
    if tasks.is_empty() {
        return "No tasks.".to_string();
    }
    tasks
        .iter()
        .map(|t| {
            let pk = t
                .project_id
                .and_then(|id| projects.iter().find(|p| p.id == id))
                .map(|p| p.project_key.as_str())
                .unwrap_or("-");
            let mut line = format!("{}  {}  {}  {}  {}", t.task_key, t.status, t.priority, pk, t.title);
            if let Some(s) = &t.scope {
                if !s.is_empty() {
                    line.push_str(&format!("  [{s}]"));
                }
            }
            if last {
                if let Ok(logs) = app.lun.logs_for("task", t.id) {
                    if let Some(e) = logs.first() {
                        line.push_str(&format!("  | {}", trunc(&e.message, 70)));
                    }
                }
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn brief(app: &App, rest: &[String], project: Option<String>) -> Result<String> {
    let o = parse_opts(rest)?;
    let mut spec = TaskListSpec::default();
    let mut status_given = false;
    let mut project = project;
    let mut i = 0;
    while i < o.rest.len() {
        let val = |i: usize, f: &str| -> Result<String> {
            o.rest
                .get(i + 1)
                .cloned()
                .ok_or_else(|| DbError::new("usage", format!("{f} needs a value")))
        };
        match o.rest[i].as_str() {
            "--all" => {}
            "--project" => {
                project = Some(val(i, "--project")?);
                i += 1;
            }
            "--status" => {
                spec.status = Some(val(i, "--status")?);
                status_given = true;
                i += 1;
            }
            "--scope" => {
                spec.scope = Some(val(i, "--scope")?);
                i += 1;
            }
            "--priority" => {
                spec.priority = Some(val(i, "--priority")?);
                i += 1;
            }
            "--assignee" => {
                spec.assignee = Some(val(i, "--assignee")?);
                i += 1;
            }
            other => {
                return Err(DbError::new(
                    "usage",
                    format!("unexpected argument '{other}' (expected: lun brief [--project P] [--status S] [--scope S] [--priority P] [--assignee A] [--all] [--last] [--json])"),
                ))
            }
        }
        i += 1;
    }
    if let Some(q) = project {
        spec.project_id = Some(resolve_project(&app.lun, &q)?.id);
    }
    let mut tasks = app.lun.list_tasks_with(&spec)?;
    if !o.all && !status_given {
        tasks.retain(|t| t.status != CLOSED_TASK_STATUS);
    }
    tasks.sort_by(|a, b| {
        (status_rank(&a.status), priority_rank(&a.priority), &a.task_key)
            .cmp(&(status_rank(&b.status), priority_rank(&b.priority), &b.task_key))
    });
    let projects = app.lun.list_projects()?;
    if o.json || o.fields.is_some() {
        let fields = Some(o.fields.clone().unwrap_or_else(|| {
            ["key", "title", "status", "priority", "project_key", "scope"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        }));
        let mut rows = Vec::new();
        for t in &tasks {
            rows.push(Value::Object(pick(task_map(t, &projects), &fields, &TASK_FIELDS)?));
        }
        return pretty(&Value::Array(rows));
    }
    Ok(brief_lines(app, &tasks, &projects, o.last))
}

// ---------------------------------------------------------------------------
// lun log ... --json / --since
// ---------------------------------------------------------------------------

fn log_ext(app: &App, rest: &[String]) -> Result<String> {
    let o = parse_opts(rest)?;
    let mut entity: Option<&str> = None;
    let mut limit: Option<usize> = None;
    for tok in &o.rest {
        let digits = tok.strip_prefix('-').filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
        if let Some(d) = digits {
            limit = Some(d.parse().map_err(|_| DbError::new("usage", format!("'{tok}' is not a valid number")))?);
        } else if tok.starts_with("HEAD~") {
            return Err(DbError::new("usage", "HEAD~N does not combine with --json/--since; use -N"));
        } else if tok.starts_with('-') {
            return Err(DbError::new("usage", format!("unexpected flag '{tok}' for lun log")));
        } else if entity.is_none() {
            entity = Some(tok);
        } else {
            return Err(DbError::new("usage", format!("unexpected argument '{tok}' for lun log")));
        }
    }
    let mut entries: Vec<LogEntry> = match entity {
        Some(q) => match resolve_entity(&app.lun, q)? {
            Entity::Project(p) => {
                let mut all = app.lun.logs_for("project", p.id)?;
                all.extend(app.lun.logs_for_project(p.id)?);
                all.sort_by(|a, b| b.id.cmp(&a.id));
                all
            }
            Entity::Task(t) => app.lun.logs_for("task", t.id)?,
        },
        None => app.lun.list_all_logs()?,
    };
    if let Some(s) = o.since {
        entries.retain(|e| e.id > s);
    }
    if let Some(n) = limit {
        entries.truncate(n);
    }
    let keys = entity_keys(app)?;
    if o.json || o.fields.is_some() {
        let mut rows = Vec::new();
        for e in &entries {
            let Value::Object(m) = log_json(e, &keys) else { continue };
            rows.push(Value::Object(pick(m, &o.fields, &LOG_FIELDS)?));
        }
        return pretty(&Value::Array(rows));
    }
    if entries.is_empty() {
        return Ok("No logs found.".to_string());
    }
    let mut out = String::new();
    for e in &entries {
        let key = keys
            .get(&(e.entity_type.clone(), e.entity_id))
            .cloned()
            .unwrap_or_else(|| "?".into());
        if o.oneline {
            out.push_str(&format!("#{}  {}  {}\n", e.id, key, log_oneline_line(e)));
        } else {
            out.push_str(&format!("#{}  {}\n", e.id, key));
            for line in task_view_entry_lines(e) {
                out.push_str(&format!("{line}\n"));
            }
            out.push('\n');
        }
    }
    Ok(out.trim_end().to_string())
}

// ---------------------------------------------------------------------------
// Bulk edits: lun task edit A,B,C ... / lun task A,B --status S
// ---------------------------------------------------------------------------

fn split_keys(q: &str) -> Vec<String> {
    q.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

fn finish_bulk(done: Vec<String>, failed: Vec<String>) -> Result<String> {
    if failed.is_empty() {
        return Ok(done.join("\n"));
    }
    let mut msg = failed.join("; ");
    if !done.is_empty() {
        msg.push_str(&format!(" (already applied: {})", done.len()));
    }
    Err(DbError::new("invalid", msg))
}

fn bulk_edit(app: &App, query: &str, args: &[String], stdin: &mut dyn BufRead) -> Result<String> {
    let keys = split_keys(query);
    if keys.is_empty() {
        return Err(DbError::new("usage", "expected: lun task edit <T-001,T-002,...> [flags]"));
    }
    if args.iter().any(|a| a == "--title" || a == "--notes" || a == "--branch") {
        return Err(DbError::new("usage", "--title/--notes/--branch are per-task; bulk edit supports --status --priority --assignee --labels --scope --project --append-notes --message"));
    }
    // Resolve everything first so a typo changes nothing.
    for k in &keys {
        resolve_task(&app.lun, k)?;
    }
    let mut args = args.to_vec();
    if !args.iter().any(|a| a == "--message") {
        args.push("--message".into());
        args.push("bulk edit".into());
    }
    let (mut done, mut failed) = (Vec::new(), Vec::new());
    for k in &keys {
        match task_edit(app, k, &args, stdin) {
            Ok(s) => done.push(s),
            Err(e) => failed.push(format!("{k}: {e}")),
        }
    }
    finish_bulk(done, failed)
}

fn bulk_status(app: &App, query: &str, args: &[String], stdin: &mut dyn BufRead) -> Result<String> {
    let keys = split_keys(query);
    let (rest, message) = parse_message_flag(args)?;
    let mut status: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--status" => {
                status = Some(
                    rest.get(i + 1)
                        .cloned()
                        .ok_or_else(|| DbError::new("usage", "--status needs a value"))?,
                );
                i += 2;
            }
            other => {
                return Err(DbError::new("usage", format!("unexpected argument '{other}' for lun task")))
            }
        }
    }
    let status = status.ok_or_else(|| DbError::new("usage", "expected --status <status>"))?;
    for k in &keys {
        resolve_task(&app.lun, k)?;
    }
    let message = message.unwrap_or_else(|| "bulk status change".to_string());
    let (mut done, mut failed) = (Vec::new(), Vec::new());
    for k in &keys {
        match task_set_status(app, k, &status, Some(message.clone()), stdin) {
            Ok(s) => done.push(s),
            Err(e) => failed.push(format!("{k}: {e}")),
        }
    }
    finish_bulk(done, failed)
}

// ---------------------------------------------------------------------------
// lun add task "<title>" [proj P] [--status S] [--priority P] [--assignee A] [--scope S] [--message M]
// ---------------------------------------------------------------------------

fn add_task_flags(app: &App, args: &[String]) -> Result<String> {
    let usage = "expected: lun add task \"<title>\" [proj \"<project>\"] [--status S] [--priority P] [--assignee A] [--scope S] [--branch B] [--message M]";
    let title = args.first().ok_or_else(|| DbError::new("usage", usage))?;
    if title.trim().is_empty() || title.starts_with("--") {
        return Err(DbError::new("usage", usage));
    }
    let (mut project_q, mut status, mut priority, mut assignee, mut scope, mut branch, mut message) =
        (None, None, None, None, None, None, None);
    let mut i = 1;
    while i < args.len() {
        let val = args
            .get(i + 1)
            .cloned()
            .ok_or_else(|| DbError::new("usage", format!("{} needs a value", args[i])));
        match args[i].as_str() {
            "proj" | "project" => project_q = Some(val?),
            "--status" => status = Some(val?),
            "--priority" => priority = Some(val?),
            "--assignee" => assignee = Some(val?),
            "--scope" => scope = Some(val?),
            "--branch" => branch = Some(val?),
            "--message" => message = Some(val?),
            other => {
                return Err(DbError::new("usage", format!("unexpected argument '{other}' ({usage})")))
            }
        }
        i += 2;
    }
    let project = match project_q {
        Some(q) => resolve_project(&app.lun, &q)?,
        None => app.lun.project_by_key("P-000")?,
    };
    let status = match status {
        Some(s) => resolve_task_status(&s).ok_or_else(|| {
            DbError::new("invalid", format!("invalid status '{s}' (expected {})", task_status_list()))
        })?,
        None => DEFAULT_TASK_STATUS.to_string(),
    };
    let priority = match priority {
        Some(p) if valid_priority(&p) => p,
        Some(p) => {
            return Err(DbError::new("invalid", format!("invalid priority '{p}' (expected low, med, or high)")))
        }
        None => "low".to_string(),
    };
    let message = message.unwrap_or_else(|| format!("add task \"{title}\" to {}", project.name));
    let task = app.lun.create_task(TaskSpec {
        title: title.clone(),
        project: Some(project.id),
        status: Some(status),
        priority: Some(priority),
        assignee: Some(assignee.unwrap_or_else(|| "me".to_string())),
        branch,
        scope,
        labels: None,
        message: Some(message.clone()),
        user: None,
    })?;
    Ok(format!(
        "Created task {} in project {}\nCommitted: {message}",
        task.task_key, project.name
    ))
}
