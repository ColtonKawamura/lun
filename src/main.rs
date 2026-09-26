//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 2: `lun init` creates `.lun/lun.db` in the current directory.
//! The rest of the CLI (`status`, `log`, ...) lands in Phase 3.

use std::env;
use std::process::ExitCode;

pub mod db;

pub use db::{Lun, LinkTarget, Project, ProjectSpec, Task, TaskSpec};

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
        Some(other) => {
            eprintln!(
                "lun v{} — command '{}' not implemented yet (Phase 2 has only `lun init`).",
                env!("CARGO_PKG_VERSION"),
                other
            );
            eprintln!("See `lun --help` for available options.");
            ExitCode::from(2)
        }
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
            println!("Phase 2: run `lun init` to create .lun/lun.db, or `lun --help`.");
            ExitCode::SUCCESS
        }
    }
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
    println!("  lun                 Show banner");
    println!("  lun init            Create .lun/lun.db in the current directory (idempotent)");
    println!("  lun --version       Print version");
    println!("  lun --help          Print this help");
    println!();
    println!("Planned (later phases):");
    println!("  lun status [project]   Project/task overview");
    println!("  lun proj add task ...  Create a task");
    println!("  lun task <id|title>    View a task");
    println!("  lun log <project|task> Show commit-style history");
}
