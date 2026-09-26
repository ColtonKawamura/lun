//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 3: `lun status`, `lun status <project>`, `lun proj add task`,
//! `lun task`, and `lun log` on top of the Phase 2 DB layer. `lun init`
//! (and the bare banner) keep working.

use std::env;
use std::process::ExitCode;

pub mod cli;
pub mod db;

pub use db::{Lun, LinkTarget, LogEntry, Project, ProjectSpec, Task, TaskSpec};

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
            print!(
                "\
    _                    _
   | |    _   _ _ __   _| | ___  _ __
   | |   | | | | '_ \\ / _` |/ _ \\| '_ \\
   | |_| | |_| | | | | (_| | (_) | | | |
   \\____/ \\__,_|_| |_|\\__,_|\\___/|_| |_|
"
            );
            println!("lun v{} — CLI-first markdown task & project tracker", env!("CARGO_PKG_VERSION"));
            println!("-----------------------------------------------------------");
            println!("Phase 3: `lun status` for the overview, `lun --help` for commands.");
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
        let conn = rusqlite::Connection::open(cwd.join(".lun/lun.db"))
            .map_err(|e| e.to_string())?;
        conn.query_row("SELECT MAX(version) FROM migrations", [], |r| {
            r.get(0)
        })
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
    println!("  lun status [name|P-00N] Global overview, or one project's status");
    println!("  lun proj add task \"<title>\"  Create a task (interactive prompts)");
    println!("  lun task <T-00N|title>    View a task (fields, labels, history)");
    println!("  lun log <project|task>    Commit-style history for a project or task");
    println!("  lun attach task <T-00N|title> /path/to/file   Attach a file (copies repo files into .lun/attachments/)");
    println!("  lun link <task|project> <key|title> \"<label>\" \"<uri>\"   Record a link");
    println!("  lun open-link <task|project> <key|title> <label>   Open a link via macOS `open`");
    println!("  lun --version             Print version");
    println!("  lun --help                Print this help");
    println!();
    println!("Planned (later phases):");
    println!("  lun (no args in TUI mode) Full-screen TUI (Phase 5+)");
}
