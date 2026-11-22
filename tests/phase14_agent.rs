//! Agent-oriented CLI additions: brief, task --brief/--log, --json/--fields,
//! log --since, bulk edits, flag-driven add task, --append-notes, --scope.

use std::process::{Command, Stdio};

fn board() -> std::path::PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("lun-p14-{}-{}-{}", std::process::id(), ts, n));
    std::fs::create_dir_all(&dir).unwrap();
    lun(&dir, &["init"]);
    lun(&dir, &["add", "proj", "Soft Pack", "--status", "active", "--message", "p"]);
    dir
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

fn seed(dir: &std::path::Path) {
    lun(dir, &["add", "task", "first", "proj", "P-001", "--status", "review", "--priority", "high", "--scope", "phys", "--message", "c1"]);
    lun(dir, &["add", "task", "second", "proj", "P-001", "--message", "c2"]);
    lun(dir, &["add", "task", "third", "--status", "closed", "--message", "c3"]);
}

#[test]
fn add_task_flags_set_everything_without_stdin() {
    let d = board();
    seed(&d);
    let v: serde_json::Value = serde_json::from_str(&lun(&d, &["task", "T-001", "--json"])).unwrap();
    assert_eq!(v["status"], "review");
    assert_eq!(v["priority"], "high");
    assert_eq!(v["scope"], "phys");
    assert_eq!(v["project_key"], "P-001");
    let (ok, _, e) = lun_raw(&d, &["add", "task", "x", "--status", "nope"]);
    assert!(!ok && e.contains("invalid status"));
    let (ok, _, e) = lun_raw(&d, &["add", "task", "x", "--priority", "urgent"]);
    assert!(!ok && e.contains("invalid priority"));
}

#[test]
fn brief_hides_closed_and_orders_by_status() {
    let d = board();
    seed(&d);
    let out = lun(&d, &["brief"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[0].starts_with("T-002  planning"), "{out}");
    assert!(lines[1].starts_with("T-001  review  high  P-001  first  [phys]"), "{out}");
    assert!(lun(&d, &["brief", "--all"]).contains("T-003"));
    assert!(lun(&d, &["brief", "--last"]).contains("| c1"));
    assert_eq!(lun(&d, &["status", "P-001", "--oneline"]).trim_end(), out.trim_end());
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["brief", "--json"])).unwrap();
    assert_eq!(j.as_array().unwrap().len(), 2);
}

#[test]
fn task_view_brief_and_log_slices() {
    let d = board();
    seed(&d);
    for n in 0..4 {
        lun(&d, &["log", "T-001", "-m", &format!("note {n}")]);
    }
    let full = lun(&d, &["task", "T-001"]);
    assert!(full.contains("note 0"));
    let brief = lun(&d, &["task", "T-001", "--brief"]);
    assert!(!brief.contains("note 0") && !brief.contains("COMMIT"), "{brief}");
    assert!(brief.contains("History: 5 entries"));
    assert!(brief.contains("PRs:"));
    let two = lun(&d, &["task", "T-001", "--log", "2"]);
    assert!(two.contains("note 3") && two.contains("note 2") && !two.contains("note 1"));
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["task", "T-001", "--json", "--log", "1"])).unwrap();
    assert_eq!(j["history_count"], 5);
    assert_eq!(j["history"].as_array().unwrap().len(), 1);
    assert_eq!(j["history"][0]["message"], "note 3");
}

#[test]
fn fields_validate_and_keep_order() {
    let d = board();
    seed(&d);
    let out = lun(&d, &["task", "T-001", "--json", "--fields", "status,key"]);
    assert!(out.find("status").unwrap() < out.find("key").unwrap(), "{out}");
    let (ok, _, e) = lun_raw(&d, &["task", "T-001", "--fields", "bogus"]);
    assert!(!ok && e.contains("unknown field"));
    let ls: serde_json::Value = serde_json::from_str(&lun(&d, &["task", "ls", "--json", "--scope", "phys"])).unwrap();
    assert_eq!(ls.as_array().unwrap().len(), 1);
    assert!(ls[0].get("notes").is_none());
    assert_eq!(lun(&d, &["task", "ls", "--fields", "key,status", "--scope", "phys"]).trim(), "T-001  review");
}

#[test]
fn log_since_and_json() {
    let d = board();
    seed(&d);
    let all: serde_json::Value = serde_json::from_str(&lun(&d, &["log", "--json"])).unwrap();
    let ids: Vec<i64> = all.as_array().unwrap().iter().map(|e| e["id"].as_i64().unwrap()).collect();
    let newest = ids[0];
    assert_eq!(lun(&d, &["log", "--since", &newest.to_string()]).trim(), "No logs found.");
    lun(&d, &["log", "T-002", "-m", "later"]);
    let new: serde_json::Value = serde_json::from_str(&lun(&d, &["log", "--since", &newest.to_string(), "--json"])).unwrap();
    assert_eq!(new.as_array().unwrap().len(), 1);
    assert_eq!(new[0]["entity"], "T-002");
    assert_eq!(new[0]["message"], "later");
    let one = lun(&d, &["log", "--since", &newest.to_string(), "--oneline"]);
    assert!(one.contains("T-002") && one.contains("later"));
    let (ok, _, e) = lun_raw(&d, &["log", "--since", "abc"]);
    assert!(!ok && e.contains("log id"));
}

#[test]
fn bulk_edit_and_status() {
    let d = board();
    seed(&d);
    lun(&d, &["task", "edit", "T-001,T-002", "--priority", "med", "--labels", "a,b"]);
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["task", "ls", "--json", "--fields", "key,priority,labels"])).unwrap();
    for t in j.as_array().unwrap().iter().filter(|t| t["key"] != "T-003") {
        assert_eq!(t["priority"], "med");
        assert_eq!(t["labels"], serde_json::json!(["a", "b"]));
    }
    lun(&d, &["task", "T-001,T-002", "--status", "waiting-on", "--message", "out"]);
    assert!(lun(&d, &["brief"]).lines().all(|l| l.contains("waiting-on")));
    // A typo anywhere changes nothing.
    let (ok, _, _) = lun_raw(&d, &["task", "edit", "T-001,T-404", "--priority", "high"]);
    assert!(!ok);
    assert!(lun(&d, &["task", "T-001", "--fields", "priority"]).contains("med"));
    let (ok, _, e) = lun_raw(&d, &["task", "edit", "T-001,T-002", "--title", "x"]);
    assert!(!ok && e.contains("per-task"));
}

#[test]
fn append_notes_and_scope_flag() {
    let d = board();
    seed(&d);
    lun(&d, &["task", "edit", "T-002", "--append-notes", "one"]);
    lun(&d, &["task", "edit", "T-002", "--append-notes", "two", "--scope", "x"]);
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["task", "T-002", "--json"])).unwrap();
    assert_eq!(j["notes"], "one\n\ntwo");
    assert_eq!(j["scope"], "x");
    lun(&d, &["task", "edit", "T-002", "--scope", "none"]);
    let j: serde_json::Value = serde_json::from_str(&lun(&d, &["task", "T-002", "--json"])).unwrap();
    assert!(j["scope"].is_null());
    let (ok, _, e) = lun_raw(&d, &["task", "edit", "T-002", "--notes", "a", "--append-notes", "b"]);
    assert!(!ok && e.contains("not both"));
}
