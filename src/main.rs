//! lun — CLI-first, markdown-formatted task and project version-control tracker.
//!
//! Phase 1: stub binary. Prints a banner and exits cleanly.
//! Future phases add `lun status`, `lun log`, the SQLite layer, and the TUI.

const BANNER: &str = r#"
    _                    _
   | |    _   _ _ __   _| | ___  _ __
   | |   | | | | '_ \ / _` |/ _ \| '_ \
   | |_| | |_| | | | | (_| | (_) | | | |
   \____/ \__,_|_| |_|\__,_|\___/|_| |_|
"#;

fn main() {
    // Placeholder CLI parsing (replaced by a real parser in Phase 3).
    // For now: any subcommand other than --help/--version just shows the stub.
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.contains(&"--version".to_string()) {
        println!("lun v{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.contains(&"--help".to_string()) || args.contains(&"help".to_string()) {
        print_help();
        return;
    }
    if !args.is_empty() {
        eprintln!("lun v{} — command '{}' not implemented yet (Phase 1 stub).", env!("CARGO_PKG_VERSION"), args.join(" "));
        eprintln!("See `lun --help` for available options.");
        std::process::exit(2);
    }

    print!("{BANNER}");
    println!("lun v{} — CLI-first markdown task & project tracker", env!("CARGO_PKG_VERSION"));
    println!("-----------------------------------------------------------");
    println!("Phase 1 skeleton. Run `lun --help` for usage.");
}

fn print_help() {
    println!("lun v{} — CLI-first markdown task & project tracker", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Usage:");
    println!("  lun                 Show banner (Phase 1 stub)");
    println!("  lun --version       Print version");
    println!("  lun --help          Print this help");
    println!();
    println!("Planned (later phases):");
    println!("  lun status [project]   Project/task overview");
    println!("  lun proj add task ...  Create a task");
    println!("  lun task <id|title>    View a task");
    println!("  lun log <project|task> Show commit-style history");
}
