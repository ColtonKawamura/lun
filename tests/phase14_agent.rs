//! Agent-oriented CLI additions: brief, task --brief/--log, --json/--fields,
//! log --since, bulk edits, flag-driven add task, --append-notes, --scope.

use std::process::{Command, Stdio};

fn board() -> (std::path::PathBuf, String) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("lun-p14-{}-{}-{}", std::process::id(), ts, n));
    std::fs::create_dir_all(&dir).unwrap();
    lun(&dir, &["init"]);
    let out = lun(&dir, &["add", "proj", "Soft Pack", "--status", "active", "--message", "p"]);
    // "Created project Soft Pack [<key>]\nCommitted: p"
    let key = out
        .lines()
        .next()
        .unwrap()
        .split('[')
        .nth(1)
        .unwrap()
        .trim_end_matches(']')
        .to_string();
    (dir, key)
}

/// Returns (success, stdout, stderr).
fn lun_raw(dir: &std::path::Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lun"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn lun(dir: &std::path::Path, args: &[&str]) -> String {
    let (ok, o, e) = lun_raw(dir, args);
    assert!(ok, "lun {args:?} failed: {e}");
    o
}

/// Parses the key out of "Created task <key> in project <name>\n...".
fn created_task_key(out: &str) -> String {
    out.lines()
        .next()
        .unwrap()
        .strip_prefix("Created task ")
        .unwrap()
        .split(" in project")
        .next()
        .unwrap()
        .to_string()
}

/// Seeds three tasks in `dir` (two in `proj_key`, one unassigned/closed) and
/// returns their generated keys in creation order.
fn seed(dir: &std::path::Path, proj_key: &str) -> (String, String, String) {
    let t1 = created_task_key(&lun(dir, &["add", "task", "first", "proj", proj_key, "--status", "review", "--priority", "high", "--scope", "phys", "--message", "c1"]));
    let t2 = created_task_key(&lun(dir, &["add", "task", "second", "proj", proj_key, "--message", "c2"]));
    let t3 = created_task_key(&lun(dir, &["add", "task", "third", "--status", "closed", "--message", "c3"]));
    (t1, t2, t3)
}

#[test]
fn add_task_flags_set_everything_without_stdin() {
    let (d, p) = board();
    let (t1, _t2, _t3) = seed(&d, &p);
    let v: serde_json::Value = serde_json::from_str(&lun(&d, &["task", &t1, "--json"])).unwrap();
    assert_eq!(v["status"], "review");
    assert_eq!(v["priority"], "high");
    assert_eq!(v["scope"], "phys");
    assert_eq!(v["project_key"], p);
    let (ok, _, e) = lun_raw(&d, &["add", "task", "x", "--status", "nope"]);
    assert!(!ok && e.contains("invalid status"));
    let (ok, _, e) = lun_raw(&d, &["add", "task", "x", "--priority", "urgent"]);
    assert!(!ok && e.contains("invalid priority"));
}

#[test]
fn brief_hides_closed_and_orders_by_status() {
    let (d, p) = board();
    let (t1, t2, t3) = seed(&d, &p);
    let out = lun(&d, &["brief"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[0].starts_with(&format!("{t2}  planning")), "{out}");
    assert!(
        lines[1].starts_with(&format!("{t1}  review  high")) && lines[1].contains("first  [phys]"),
        "{out}"
    );
    assert!(lun(&d, &["brief", "--all"]).contains(&t3));
    assert!(lun(&d, &["brief", "--last"]).contains("| c1"));
    assert_eq!(lun(&d, &["status", &p, "--oneline"]).trim_end(), out.trim_end());
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["brief", "--json"])).unwrap();
    assert_eq!(j.as_array().unwrap().len(), 2);
}

#[test]
fn task_view_brief_and_log_slices() {
    let (d, p) = board();
    let (t1, _t2, _t3) = seed(&d, &p);
    for n in 0..4 {
        lun(&d, &["log", &t1, "-m", &format!("note {n}")]);
    }
    let full = lun(&d, &["task", &t1]);
    assert!(full.contains("note 0"));
    let brief = lun(&d, &["task", &t1, "--brief"]);
    assert!(!brief.contains("note 0") && !brief.contains("COMMIT"), "{brief}");
    assert!(brief.contains("History: 5 entries"));
    assert!(brief.contains("PRs:"));
    let two = lun(&d, &["task", &t1, "--log", "2"]);
    assert!(two.contains("note 3") && two.contains("note 2") && !two.contains("note 1"));
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["task", &t1, "--json", "--log", "1"])).unwrap();
    assert_eq!(j["history_count"], 5);
    assert_eq!(j["history"].as_array().unwrap().len(), 1);
    assert_eq!(j["history"][0]["message"], "note 3");
}

#[test]
fn fields_validate_and_keep_order() {
    let (d, p) = board();
    let (t1, _t2, _t3) = seed(&d, &p);
    let out = lun(&d, &["task", &t1, "--json", "--fields", "status,key"]);
    assert!(out.find("status").unwrap() < out.find("key").unwrap(), "{out}");
    let (ok, _, e) = lun_raw(&d, &["task", &t1, "--fields", "bogus"]);
    assert!(!ok && e.contains("unknown field"));
    let ls: serde_json::Value = serde_json::from_str(&lun(&d, &["task", "ls", "--json", "--scope", "phys"])).unwrap();
    assert_eq!(ls.as_array().unwrap().len(), 1);
    assert!(ls[0].get("notes").is_none());
    assert_eq!(
        lun(&d, &["task", "ls", "--fields", "key,status", "--scope", "phys"]).trim(),
        format!("{t1}  review")
    );
}

#[test]
fn log_since_and_json() {
    let (d, p) = board();
    let (_t1, t2, _t3) = seed(&d, &p);
    let all: serde_json::Value = serde_json::from_str(&lun(&d, &["log", "--json"])).unwrap();
    let ids: Vec<i64> = all.as_array().unwrap().iter().map(|e| e["id"].as_i64().unwrap()).collect();
    let newest = ids[0];
    assert_eq!(lun(&d, &["log", "--since", &newest.to_string()]).trim(), "No logs found.");
    lun(&d, &["log", &t2, "-m", "later"]);
    let new: serde_json::Value = serde_json::from_str(&lun(&d, &["log", "--since", &newest.to_string(), "--json"])).unwrap();
    assert_eq!(new.as_array().unwrap().len(), 1);
    assert_eq!(new[0]["entity"], t2);
    assert_eq!(new[0]["message"], "later");
    let one = lun(&d, &["log", "--since", &newest.to_string(), "--oneline"]);
    assert!(one.contains(&t2) && one.contains("later"));
    let (ok, _, e) = lun_raw(&d, &["log", "--since", "abc"]);
    assert!(!ok && e.contains("log id"));
}

#[test]
fn bulk_edit_and_status() {
    let (d, p) = board();
    let (t1, t2, t3) = seed(&d, &p);
    lun(&d, &["task", "edit", &format!("{t1},{t2}"), "--priority", "med", "--labels", "a,b"]);
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["task", "ls", "--json", "--fields", "key,priority,labels"])).unwrap();
    for t in j.as_array().unwrap().iter().filter(|t| t["key"] != t3.as_str()) {
        assert_eq!(t["priority"], "med");
        assert_eq!(t["labels"], serde_json::json!(["a", "b"]));
    }
    lun(&d, &["task", &format!("{t1},{t2}"), "--status", "waiting-on", "--message", "out"]);
    assert!(lun(&d, &["brief"]).lines().all(|l| l.contains("waiting-on")));
    // A typo anywhere changes nothing.
    let (ok, _, _) = lun_raw(&d, &["task", "edit", &format!("{t1},T-404"), "--priority", "high"]);
    assert!(!ok);
    assert!(lun(&d, &["task", &t1, "--fields", "priority"]).contains("med"));
    let (ok, _, e) = lun_raw(&d, &["task", "edit", &format!("{t1},{t2}"), "--title", "x"]);
    assert!(!ok && e.contains("per-task"));
}

#[test]
fn append_notes_and_scope_flag() {
    let (d, p) = board();
    let (_t1, t2, _t3) = seed(&d, &p);
    lun(&d, &["task", "edit", &t2, "--append-notes", "one"]);
    lun(&d, &["task", "edit", &t2, "--append-notes", "two", "--scope", "x"]);
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["task", &t2, "--json"])).unwrap();
    assert_eq!(j["notes"], "one\n\ntwo");
    assert_eq!(j["scope"], "x");
    lun(&d, &["task", "edit", &t2, "--scope", "none"]);
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["task", &t2, "--json"])).unwrap();
    assert!(j["scope"].is_null());
    let (ok, _, e) = lun_raw(&d, &["task", "edit", &t2, "--notes", "a", "--append-notes", "b"]);
    assert!(!ok && e.contains("not both"));
}
