//! Board root discovery, git-style.
//!
//! A board is a directory containing `.lun/lun.db`. Like git finds a repo,
//! lun looks for the board in the current directory first (the behaviour
//! before discovery existed), then in each parent directory, nearest wins.
//! The walk stops at `$HOME` (it never looks in `$HOME` itself or above,
//! unless the starting directory *is* `$HOME`) so a stray `~/.lun` cannot
//! capture every project below it. `lun init` never walks: it always
//! creates the board in the current directory.

use std::env;
use std::path::{Path, PathBuf};

use crate::db::DbError;

/// Does `dir` hold a board (`<dir>/.lun/lun.db`)?
pub fn is_board(dir: &Path) -> bool {
    dir.join(".lun").join("lun.db").is_file()
}

/// Find the board root for `start`: `start` itself if it is a board,
/// otherwise the nearest ancestor that is, never reaching `ceiling`
/// (normally `$HOME`) or anything above it.
pub fn find_root(start: &Path, ceiling: Option<&Path>) -> Option<PathBuf> {
    if is_board(start) {
        return Some(start.to_path_buf());
    }
    for dir in start.ancestors().skip(1) {
        if Some(dir) == ceiling {
            return None;
        }
        if is_board(dir) {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// `$HOME` as the search ceiling. Canonicalized, because `current_dir()` is
/// the physical path and a symlinked `$HOME` would otherwise never match.
fn home_ceiling() -> Option<PathBuf> {
    let home = PathBuf::from(env::var_os("HOME")?);
    if !home.is_absolute() {
        return None;
    }
    Some(std::fs::canonicalize(&home).unwrap_or(home))
}

/// The error for "no board here or above".
pub fn not_found_error(start: &Path) -> DbError {
    DbError::new(
        "not-initialized",
        format!(
            "no lun board found in {} or any parent directory (the search stops at $HOME); \
             run `lun init` first, in the directory that should hold the board",
            start.display()
        ),
    )
}

/// Resolve the board root for the process's current directory.
/// Returns `(root, cwd)` so callers can tell whether discovery walked up.
pub fn from_cwd() -> Result<(PathBuf, PathBuf), DbError> {
    let cwd = env::current_dir()
        .map_err(|e| DbError::new("io", format!("resolving CWD: {e}")))?;
    let root = find_root(&cwd, home_ceiling().as_deref()).ok_or_else(|| not_found_error(&cwd))?;
    Ok((root, cwd))
}
