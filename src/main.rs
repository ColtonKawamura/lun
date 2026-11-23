//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 3: `lun status`, `lun status <project>`, `lun add task`,
//! `lun task`, and `lun log` on top of the Phase 2 DB layer.
//! Phase 4: `lun attach`, `lun link`, `lun open-link` (Mac linking).
//! Phase 5: bare `lun` launches the full-screen TUI when stdout is a TTY
//! (piped output keeps the plain banner).
//! Phase 9: `lun pr new/show/ls/merge` (GitHub-style PRs + git glue).
//! Phase 10: `lun serve` — HTTP server + PWA on Tailscale.

use std::env;
use std::process::ExitCode;

pub mod cli;
pub mod db;
pub mod root;
pub mod server;
pub mod tui;

pub use db::{
    AttachmentTarget, LinkTarget, LogEntry, Lun, Pr, Project, ProjectDeleteMode, ProjectDeleted, TaskDeleted,
    ProjectSpec, Task, TaskListSpec,
    TaskSort, TaskSpec, TaskUpdateSpec,
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("complete") {
        let app = root::from_cwd()
            .ok()
            .and_then(|(root, _)| cli::App::open(&root).ok());
        let out = cli::complete_output(app.as_ref(), &args[1..]);
        if !out.is_empty() {
            println!("{out}");
        }
        return ExitCode::SUCCESS;
    }

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
        Some("serve") => run_serve(&args[1..]),
        Some("root") => match root::from_cwd() {
            Ok((root, _)) => {
                println!("{}", root.display());
                ExitCode::SUCCESS
            }
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
                let root = match resolve_board() {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("lun: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                match tui::term::launch(&root, env!("CARGO_PKG_VERSION")) {
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

/// Find the board for the current directory (here or the nearest parent),
/// and say so on stderr when it is not the current directory, so a command
/// never silently lands on a board the user did not expect.
fn resolve_board() -> Result<std::path::PathBuf, String> {
    let (root, cwd) = root::from_cwd().map_err(|e| e.to_string())?;
    if root != cwd {
        eprintln!("lun: using board at {}", root.display());
    }
    Ok(root)
}

/// Open the board's `.lun/lun.db` (current directory or nearest parent).
fn open_app() -> Result<cli::App, String> {
    let root = resolve_board()?;
    cli::App::open(&root).map_err(|e| e.to_string())
}

/// `lun init`: create `.lun/lun.db` in the current working directory (never
/// a parent), run pending migrations, and print a short report. Idempotent
/// on re-run.
fn init_db() -> Result<(), String> {
    let cwd = env::current_dir().map_err(|e| format!("resolving CWD: {e}"))?;
    if !root::is_board(&cwd) {
        if let Ok((parent, _)) = root::from_cwd() {
            eprintln!(
                "lun init: note: {} is already a board; this creates a separate, nested board here",
                parent.display()
            );
        }
    }
    let lun = Lun::init(&cwd).map_err(|e| e.to_string())?;
    let version = lun.schema_version().map_err(|e| e.to_string())?;

    if version == db::CURRENT_VERSION {
        println!("lun init: .lun/lun.db ready (schema v{version})");
    }
    println!("lun init: re-run anytime — migrations are idempotent.");
    Ok(())
}

fn print_help() {
    println!("{}", cli::help_text(env!("CARGO_PKG_VERSION")));
}

/// `lun serve` — Start the HTTP server + PWA.
fn run_serve(args: &[String]) -> ExitCode {
    let bind = args.first().map(String::as_str).unwrap_or("100.71.78.78:9120");
    // serve never creates a board: it needs an existing one (here or the
    // nearest parent), so a wrong working directory fails loudly.
    let root = match root::from_cwd() {
        Ok((root, _)) => root,
        Err(e) => {
            eprintln!("lun: {e}");
            return ExitCode::FAILURE;
        }
    };
    let data_dir = root.join(".lun");
    eprintln!("lun serve: board {}", root.display());

    // Open DB in WAL mode
    let lun = match Lun::init(&root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("lun: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = lun.set_pragma("PRAGMA busy_timeout = 5000") {
        eprintln!("lun serve: warning: {e}");
    }

    // Set up tracing
    tracing_subscriber::fmt::init();

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("lun serve: creating tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = rt.block_on(server::serve(lun, data_dir, bind)) {
        eprintln!("lun serve: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
