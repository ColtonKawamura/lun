//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 3: `lun status`, `lun status <project>`, `lun add task`,
//! `lun task`, and `lun log` on top of the Phase 2 DB layer.
//! Phase 4: `lun attach`, `lun link`, `lun open-link` (Mac linking).
//! Phase 5: bare `lun` launches the full-screen TUI when stdout is a TTY
//! (piped output keeps the plain banner).
//! Phase 9: `lun pr new/show/ls/merge` (GitHub-style PRs + git glue).

use std::env;
use std::process::ExitCode;

pub mod cli;
pub mod db;
pub mod tui;

pub use db::{
    AttachmentTarget, LinkTarget, LogEntry, Lun, Pr, Project, ProjectSpec, Task, TaskListSpec,
    TaskSort, TaskSpec, TaskUpdateSpec,
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.contains(&"--version".to_string()) {
        println!("lun v{}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.contains(&"--help".to_string()) || args.contains(&"help".to_string()) {
        print_help();
        return ExitCode::SUCCESS;
    }

    match args.first().map(String::as_str) {
        Some("init") => match init_db() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("lun: {e}");
                ExitCode::FAILURE
            }
        },
        Some(_) => match open_app() {
            Ok(app) => cli::run(&app, &args),
            Err(e) => {
                eprintln!("lun: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            // Phase 5: bare `lun` opens the full-screen TUI when stdout is a
            // TTY; piped/redirected output keeps the plain banner so scripts
            // can still capture `lun` output.
            use std::io::IsTerminal;
            if std::io::stdout().is_terminal() {
                let cwd = match env::current_dir() {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("lun: resolving CWD: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                match tui::term::launch(&cwd, env!("CARGO_PKG_VERSION")) {
                    Ok(0) => return ExitCode::SUCCESS,
                    Ok(code) => return ExitCode::from(code as u8),
                    Err(e) => {
                        eprintln!("lun: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            print!(
                "\
    _                    _
   | |    _   _ _ __   _| | ___  _ __
   | |   | | | | '_ \\ / _` |/ _ \\| '_ \\
   | |_| | |_| | | | | (_| | (_) | | | |
   \\____/ \\__,_|_| |_|\\__,_|\\___/|_| |_|
"
            );
            println!(
                "lun v{} — CLI-first markdown task & project tracker",
                env!("CARGO_PKG_VERSION")
            );
            println!("-----------------------------------------------------------");
            println!("Run `lun` in a terminal for the full-screen TUI (Phase 5);");
            println!("`lun --help` for CLI commands.");
            ExitCode::SUCCESS
        }
    }
}

/// Open `.lun/lun.db` in the current working directory.
fn open_app() -> Result<cli::App, String> {
    let cwd = env::current_dir().map_err(|e| format!("resolving CWD: {e}"))?;
    cli::App::open(&cwd).map_err(|e| e.to_string())
}

/// `lun init`: create `.lun/lun.db` in the current working directory, run
/// pending migrations, and print a short report. Idempotent on re-run.
fn init_db() -> Result<(), String> {
    let cwd = env::current_dir().map_err(|e| format!("resolving CWD: {e}"))?;
    let lun = Lun::init(&cwd).map_err(|e| e.to_string())?;

    let version: i64 = {
        let conn =
            rusqlite::Connection::open(cwd.join(".lun/lun.db")).map_err(|e| e.to_string())?;
        conn.query_row("SELECT MAX(version) FROM migrations", [], |r| r.get(0))
            .map_err(|e| e.to_string())?
    };

    if version == db::CURRENT_VERSION {
        println!("lun init: .lun/lun.db ready (schema v{version})");
    }
    println!("lun init: re-run anytime — migrations are idempotent.");
    drop(lun);
    Ok(())
}

fn print_help() {
    println!(
        "lun v{} — CLI-first markdown task & project tracker",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("Usage:");
    println!("  lun                     Show banner");
    println!("  lun init                Create .lun/lun.db in the current directory (idempotent)");
    println!("  lun status [name|P-00N] [--board] Projects overview, or one project's tasks/board");
    println!("  lun new proj \"<name>\"  Create a project");
    println!(
        "  lun add task \"<title>\" [proj \"<project>\"]  Create a task (interactive prompts)"
    );
    println!("  lun move \"<task>\" \"<project>\"   Move a task to a project");
    println!("  lun task <T-00N|title>    View a task (fields, labels, history)");
    println!("  lun task <task> --status <todo|doing|follow-up|blocked|done>   Update task status");
    println!("  lun task ls [filters]     List tasks (--project/--status/--priority/--assignee/--sort/--all)");
    println!("  lun task edit <task> [--field value]   Edit task fields");
    println!("  lun proj <project> --status <active|inactive>   Update project status");
    println!("  lun task complete|reopen|archive <task>   Update task lifecycle");
    println!("  lun log <project|task>    Commit-style history for a project or task");
    println!("  lun attach <task|project> <key|title> /path/to/file   Attach a file (copies repo files into .lun/attachments/)");
    println!("  lun attach ls <task|project> <key|title>   List attachments");
    println!(
        "  lun attach open|rm <task|project> <key|title> <filename|id>   Open/remove attachments"
    );
    println!("  lun link <task|project> <key|title> \"<label>\" \"<uri>\"   Record a link");
    println!("  lun open-link <task|project> <key|title> <label>   Open a link via macOS `open`");
    println!("  lun open-uri <uri> [--on <task|project> <key|title>]   Open any URI (used by the nvim plugin; logs LINK_OPENED with --on)");
    println!("  lun pr new <T-00N|title> [--from <branch>] [--to <branch>]   Open a PR (defaults: task's branch -> main)");
    println!("  lun pr show <PR-00N|task>   View a PR (branches, status, PR log history)");
    println!("  lun pr ls               List open and merged PRs");
    println!(
        "  lun pr merge <PR-00N|task>   Merge a PR (task -> done; runs `git merge` when possible)"
    );
    println!("  lun --version             Print version");
    println!("  lun --help                Print this help");
    println!();
    println!("Planned (later phases):");
    println!("  lun (no args in TUI mode) Full-screen TUI (Phase 5+)");
}
