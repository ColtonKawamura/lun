//! Git-style board discovery: a command run anywhere under a board's
//! directory uses that board (nearest wins, stops at $HOME), `lun init`
//! never walks, and nothing is ever created implicitly.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use lun::root::{display_path, find_root, is_board};

static N: AtomicUsize = AtomicUsize::new(0);

/// A fresh canonical temp directory (macOS temp dirs sit behind a symlink).
fn tmp(name: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("lun-root-{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::canonicalize(&dir).unwrap()
}

/// Make `dir` a board the way `lun init` does (but without the binary).
fn fake_board(dir: &Path) {
    fs::create_dir_all(dir.join(".lun")).unwrap();
    fs::write(dir.join(".lun/lun.db"), b"").unwrap();
}

fn sub(dir: &Path, rel: &str) -> PathBuf {
    let d = dir.join(rel);
    fs::create_dir_all(&d).unwrap();
    d
}

// ---- find_root (library) ----

#[test]
fn board_dir_itself_is_found() {
    let t = tmp("self");
    fake_board(&t);
    assert_eq!(find_root(&t, None), Some(t.clone()));
}

#[test]
fn deep_subdirectory_finds_parent_board() {
    let t = tmp("deep");
    fake_board(&t);
    let d = sub(&t, "a/b/c/d");
    assert_eq!(find_root(&d, None), Some(t.clone()));
}

#[test]
fn nearest_board_wins() {
    let t = tmp("nested");
    fake_board(&t);
    let inner = sub(&t, "proj");
    fake_board(&inner);
    let deep = sub(&inner, "src/x");
    assert_eq!(find_root(&deep, None), Some(inner.clone()));
    assert_eq!(find_root(&sub(&t, "other"), None), Some(t.clone()));
}

#[test]
fn sibling_boards_never_see_each_other() {
    let t = tmp("sib");
    let work = sub(&t, "work");
    let personal = sub(&t, "personal");
    fake_board(&work);
    fake_board(&personal);
    assert_eq!(find_root(&sub(&work, "a/b"), None), Some(work.clone()));
    assert_eq!(find_root(&sub(&personal, "a/b"), None), Some(personal.clone()));
    // the parent of both is no board at all
    assert_eq!(find_root(&t, Some(t.parent().unwrap())), None);
}

#[test]
fn no_board_anywhere_is_none() {
    let t = tmp("none");
    let d = sub(&t, "x/y");
    // ceiling above the temp dir keeps the walk from escaping into the real fs
    assert_eq!(find_root(&d, Some(t.parent().unwrap())), None);
}

#[test]
fn stray_dot_lun_without_db_is_ignored() {
    // e.g. a leftover `.lun/attachments/` must not count as a board
    let t = tmp("stray");
    fake_board(&t);
    let d = sub(&t, "sub");
    fs::create_dir_all(d.join(".lun/attachments")).unwrap();
    assert!(!is_board(&d));
    assert_eq!(find_root(&d, None), Some(t.clone()));
}

#[test]
fn dot_lun_db_as_directory_is_not_a_board() {
    let t = tmp("dbdir");
    fs::create_dir_all(t.join(".lun/lun.db")).unwrap();
    assert!(!is_board(&t));
}

#[test]
fn ceiling_blocks_boards_at_or_above_it() {
    let t = tmp("ceil");
    fake_board(&t); // plays the role of a stray ~/.lun
    let project = sub(&t, "project/src");
    assert_eq!(find_root(&project, Some(&t)), None);
    // ...but a board strictly below the ceiling is still found
    let p = sub(&t, "project");
    fake_board(&p);
    assert_eq!(find_root(&project, Some(&t)), Some(p));
}

#[test]
fn starting_at_the_ceiling_still_uses_its_own_board() {
    // a board directly in $HOME keeps working when run from $HOME
    let t = tmp("ceil-self");
    fake_board(&t);
    assert_eq!(find_root(&t, Some(&t)), Some(t.clone()));
}

// ---- the binary ----

fn lun_in(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lun"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .output()
        .unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn init(dir: &Path, home: &Path) {
    let o = lun_in(dir, home, &["init"]);
    assert!(o.status.success(), "init failed: {}", err(&o));
}

#[test]
fn cli_runs_from_subdirectory_against_parent_board() {
    let home = tmp("cli-sub");
    let board = sub(&home, "repos");
    let deep = sub(&board, "proj/src");
    init(&board, &home);

    let o = lun_in(&deep, &home, &["add", "proj", "From A Subdir", "--status", "active"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("using board at"), "walk-up must be announced: {}", err(&o));

    // it landed in the parent board, and no stray board was created below
    assert!(!deep.join(".lun").exists());
    assert!(!board.join("proj/.lun").exists());
    let s = lun_in(&board, &home, &["status"]);
    assert!(out(&s).contains("From A Subdir"), "{}", out(&s));
    // at the board itself there is no note
    assert!(!err(&s).contains("using board at"), "{}", err(&s));
}

#[test]
fn lun_root_prints_board_dir() {
    let home = tmp("cli-root");
    let board = sub(&home, "repos");
    let deep = sub(&board, "a/b");
    init(&board, &home);
    let o = lun_in(&deep, &home, &["root"]);
    assert!(o.status.success());
    assert_eq!(out(&o).trim(), board.to_str().unwrap());
    assert_eq!(err(&o), "", "root must be quiet on stderr");
}

#[test]
fn sibling_boards_are_isolated_through_the_cli() {
    let home = tmp("cli-sib");
    let work = sub(&home, "repos");
    let personal = sub(&home, "personal-board");
    init(&work, &home);
    init(&personal, &home);
    lun_in(&work, &home, &["add", "proj", "Work Only", "--status", "active"]);
    lun_in(&personal, &home, &["add", "proj", "Home Only", "--status", "active"]);

    let w = out(&lun_in(&sub(&work, "x"), &home, &["status"]));
    let p = out(&lun_in(&sub(&personal, "y"), &home, &["status"]));
    assert!(w.contains("Work Only") && !w.contains("Home Only"), "{w}");
    assert!(p.contains("Home Only") && !p.contains("Work Only"), "{p}");
}

#[test]
fn no_board_fails_loudly_and_creates_nothing() {
    let home = tmp("cli-none");
    let d = sub(&home, "empty/dir");
    for args in [&["status"][..], &["root"], &["add", "proj", "X"], &["brief"]] {
        let o = lun_in(&d, &home, args);
        assert!(!o.status.success(), "{args:?} should fail");
        assert!(err(&o).contains("lun init"), "{args:?}: {}", err(&o));
    }
    assert!(!d.join(".lun").exists());
    assert!(!home.join("empty/.lun").exists());
}

#[test]
fn serve_without_a_board_fails_and_does_not_create_one() {
    let home = tmp("cli-serve");
    let d = sub(&home, "nowhere");
    let o = lun_in(&d, &home, &["serve", "127.0.0.1:0"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("lun init"), "{}", err(&o));
    assert!(!d.join(".lun").exists(), "serve must never create a board");
}

#[test]
fn init_never_walks_up_and_warns_about_nesting() {
    let home = tmp("cli-init");
    let board = sub(&home, "repos");
    init(&board, &home);
    let inner = sub(&board, "lun");

    let o = lun_in(&inner, &home, &["init"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("nested board"), "{}", err(&o));
    assert!(inner.join(".lun/lun.db").is_file());

    // nearest wins from now on, and the two boards stay separate
    lun_in(&inner, &home, &["add", "proj", "Inner", "--status", "active"]);
    assert!(out(&lun_in(&inner, &home, &["status"])).contains("Inner"));
    assert!(!out(&lun_in(&board, &home, &["status"])).contains("Inner"));
}

#[test]
fn init_twice_in_same_dir_is_quiet_and_idempotent() {
    let home = tmp("cli-init2");
    let board = sub(&home, "repos");
    init(&board, &home);
    let o = lun_in(&board, &home, &["init"]);
    assert!(o.status.success());
    assert!(!err(&o).contains("nested"), "{}", err(&o));
}

#[test]
fn stray_board_in_home_does_not_capture_other_directories() {
    let home = tmp("cli-ceil");
    init(&home, &home); // a board directly in $HOME
    let elsewhere = sub(&home, "projects/thing");
    let o = lun_in(&elsewhere, &home, &["status"]);
    assert!(!o.status.success(), "must not walk up into $HOME: {}", out(&o));
    // but $HOME itself keeps working
    let o = lun_in(&home, &home, &["status"]);
    assert!(o.status.success(), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn symlinked_home_still_acts_as_ceiling() {
    let real = tmp("cli-symhome-real");
    let link = real.parent().unwrap().join(format!(
        "lun-root-symlink-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    init(&real, &link); // board at the (real) home
    let d = sub(&real, "projects/x");
    // HOME is given through the symlink, cwd is the physical path
    let o = lun_in(&d, &link, &["status"]);
    assert!(!o.status.success(), "ceiling must hold through a symlinked HOME");
    let _ = fs::remove_file(&link);
}

#[test]
fn attach_from_subdirectory_stores_in_board_attachments() {
    let home = tmp("cli-attach");
    let board = sub(&home, "repos");
    let deep = sub(&board, "proj/notes");
    init(&board, &home);
    lun_in(&board, &home, &["add", "proj", "P", "--status", "active"]);
    let o = lun_in(
        &board,
        &home,
        &["add", "task", "A task", "proj", "P", "--status", "planning", "--priority", "med",
          "--assignee", "t", "--message", "m"],
    );
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    let key = out(&o)
        .split_whitespace()
        .find(|w| w.starts_with("T-"))
        .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric()).to_string())
        .expect("task key");

    let file = deep.join("note.txt");
    fs::write(&file, "hello").unwrap();
    let o = lun_in(&deep, &home, &["attach", "task", &key, file.to_str().unwrap()]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    assert!(board.join(".lun/attachments").is_dir(), "attachments belong to the board");
    assert!(!deep.join(".lun").exists(), "no stray .lun in the subdirectory");
    let copied = fs::read_dir(board.join(".lun/attachments")).unwrap().count();
    assert_eq!(copied, 1);
}

// ---- board directory shown in the web UI ----

#[test]
fn display_path_uses_tilde_under_home() {
    let home = Path::new("/home/colton");
    assert_eq!(display_path(Path::new("/home/colton/repos"), Some(home)), "~/repos/");
    assert_eq!(
        display_path(Path::new("/home/colton/personal-board"), Some(home)),
        "~/personal-board/"
    );
    assert_eq!(display_path(Path::new("/home/colton/a/b"), Some(home)), "~/a/b/");
    assert_eq!(display_path(home, Some(home)), "~/");
}

#[test]
fn display_path_keeps_full_path_outside_home() {
    let home = Path::new("/home/colton");
    assert_eq!(display_path(Path::new("/srv/boards/work"), Some(home)), "/srv/boards/work/");
    // a sibling that merely shares a name prefix is not under home
    assert_eq!(display_path(Path::new("/home/colton2/x"), Some(home)), "/home/colton2/x/");
    assert_eq!(display_path(Path::new("/srv/boards/work"), None), "/srv/boards/work/");
    assert_eq!(display_path(Path::new("/"), None), "/");
}

/// Minimal HTTP/1.0 GET returning (status, body).
fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok()?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).ok()?;
    let status = raw.split_whitespace().nth(1)?.parse().ok()?;
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    Some((status, body))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// Run `lun serve` from `cwd`, fetch `/api/board`, then stop it.
fn board_api(cwd: &Path, home: &Path) -> serde_json::Value {
    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_lun"))
        .args(["serve", &format!("127.0.0.1:{port}")])
        .current_dir(cwd)
        .env("HOME", home)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut got = None;
    for _ in 0..100 {
        if let Some(r) = http_get(port, "/api/board") {
            got = Some(r);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let (status, body) = got.expect("server never answered");
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

#[test]
fn serve_reports_its_board_directory_with_tilde() {
    let home = tmp("api-board");
    let work = sub(&home, "repos");
    let personal = sub(&home, "personal-board");
    init(&work, &home);
    init(&personal, &home);

    let w = board_api(&work, &home);
    assert_eq!(w["display"], "~/repos/");
    assert_eq!(w["root"], work.to_str().unwrap());

    // started from a subdirectory: reports the board root, not the cwd
    let p = board_api(&sub(&personal, "deep/er"), &home);
    assert_eq!(p["display"], "~/personal-board/");
    assert_eq!(p["root"], personal.to_str().unwrap());
}

#[test]
fn serve_outside_home_shows_full_path() {
    let home = tmp("api-board-out-home");
    let elsewhere = tmp("api-board-out-board"); // not under `home`
    init(&elsewhere, &home);
    let v = board_api(&elsewhere, &home);
    assert_eq!(v["display"], format!("{}/", elsewhere.display()));
}

#[test]
fn index_page_has_the_board_label_slot() {
    // the static page must carry the element the script fills in
    let html = include_str!("../static/index.html");
    assert!(html.contains(r#"id="boardDir""#));
    assert!(html.contains("fetch('/api/board')"));
}
