//! Schema v11 tests: every log entry carries a git-style hash chained to the
//! previous entry of the same entity, each note diff points at the log entry
//! that recorded it (so a hash recovers the old notes), and edits that give no
//! message log what they changed (`edit T-001; update note`).

use lun::{Lun, TaskSpec, TaskUpdateSpec};
use std::collections::HashSet;

fn temp_root(name: &str) -> std::path::PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lun-hash-{}-{}-{}", name, std::process::id(), ts));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn board(name: &str) -> (Lun, std::path::PathBuf) {
    let root = temp_root(name);
    let lun = Lun::init(&root).unwrap();
    (lun, root)
}

fn new_task(lun: &Lun) -> lun::Task {
    lun.create_task(TaskSpec {
        title: "hash test".into(),
        ..Default::default()
    })
    .unwrap()
}

fn is_hash(h: &str) -> bool {
    h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())
}

#[test]
fn fresh_board_hashes_the_seed_entry() {
    let (lun, root) = board("seed");
    assert_eq!(lun.schema_version().unwrap(), lun::db::CURRENT_VERSION);
    let seed = lun.logs_for("project", 1).unwrap();
    assert_eq!(seed.len(), 1);
    assert!(is_hash(&seed[0].hash), "seed hash: {:?}", seed[0].hash);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn identical_entries_in_the_same_second_get_different_hashes() {
    let (lun, root) = board("unique");
    let task = new_task(&lun);
    for _ in 0..5 {
        lun.insert_log("task", task.id, "me", "COMMIT", "did: same message", "{}")
            .unwrap();
    }
    let logs = lun.logs_for("task", task.id).unwrap();
    let hashes: HashSet<&str> = logs.iter().map(|l| l.hash.as_str()).collect();
    assert_eq!(hashes.len(), logs.len(), "hashes must be unique per entry");
    assert!(logs.iter().all(|l| is_hash(&l.hash)));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn log_by_hash_resolves_full_and_short_hashes() {
    let (lun, root) = board("lookup");
    let task = new_task(&lun);
    lun.insert_log("task", task.id, "me", "COMMIT", "did: find me", "{}")
        .unwrap();
    let entry = lun.logs_for("task", task.id).unwrap().remove(0);

    assert_eq!(lun.log_by_hash(&entry.hash).unwrap().id, entry.id);
    assert_eq!(lun.log_by_hash(&entry.hash[..7]).unwrap().id, entry.id);
    assert_eq!(
        lun.log_by_hash(&entry.hash[..7].to_uppercase()).unwrap().id,
        entry.id,
        "lookup is case-insensitive"
    );

    // Too short / not hex / unknown are errors, not guesses.
    assert_eq!(lun.log_by_hash("ab").unwrap_err().kind(), "usage");
    assert_eq!(lun.log_by_hash("zzzzzzz").unwrap_err().kind(), "usage");
    assert_eq!(lun.log_by_hash("ffffffffffff").unwrap_err().kind(), "not-found");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn note_edit_is_recoverable_from_its_log_hash() {
    let (lun, root) = board("recover");
    let task = new_task(&lun);

    lun.set_notes(task.id, "first draft", None, None).unwrap();
    lun.set_notes(task.id, "second draft\nwith more", None, None)
        .unwrap();
    // An unchanged save records nothing.
    lun.set_notes(task.id, "second draft\nwith more", None, None)
        .unwrap();

    let diffs = lun.get_note_diffs(task.id).unwrap();
    assert_eq!(diffs.len(), 2);
    assert_eq!(diffs[0].old_text, "");
    assert_eq!(diffs[1].old_text, "first draft");

    // The diff knows its log entry, and the hash of that entry finds it again.
    let hash = diffs[1].log_hash.clone().expect("diff is linked to a log entry");
    assert!(is_hash(&hash));
    let entry = lun.log_by_hash(&hash[..7]).unwrap();
    assert_eq!(entry.id, diffs[1].log_id.unwrap());
    let found = lun.note_diff_for_log(entry.id).unwrap().expect("entry edited notes");
    assert_eq!(found.old_text, "first draft");

    // Entries that did not edit notes have no diff.
    lun.insert_log("task", task.id, "me", "COMMIT", "did: x", "{}").unwrap();
    let plain = lun.logs_for("task", task.id).unwrap().remove(0);
    assert!(lun.note_diff_for_log(plain.id).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn update_task_notes_edit_links_diff_to_log_entry() {
    let (lun, root) = board("update");
    let task = new_task(&lun);
    lun.update_task(
        task.id,
        TaskUpdateSpec {
            notes: Some("v1".into()),
            user: Some("web".into()),
            ..Default::default()
        },
    )
    .unwrap();
    lun.update_task(
        task.id,
        TaskUpdateSpec {
            notes: Some("v2".into()),
            user: Some("web".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let diffs = lun.get_note_diffs(task.id).unwrap();
    assert_eq!(diffs.len(), 2);
    assert_eq!(diffs[1].old_text, "v1");
    assert!(diffs.iter().all(|d| d.log_hash.as_deref().is_some_and(is_hash)));
    assert_eq!(diffs[1].user, "web");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn diffs_do_not_litter_hunks_with_no_newline_markers() {
    let (lun, root) = board("nonl");
    let task = new_task(&lun);
    lun.set_notes(task.id, "line one\nline two", None, None).unwrap();
    lun.set_notes(task.id, "line one\nline TWO\nline three", None, None)
        .unwrap();
    for d in lun.get_note_diffs(task.id).unwrap() {
        assert!(!d.diff.contains("No newline"), "{}", d.diff);
    }
    // A change that is *only* the trailing newline still records something.
    lun.set_notes(task.id, "line one\nline TWO\nline three\n", None, None)
        .unwrap();
    assert_eq!(lun.get_note_diffs(task.id).unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&root);
}

fn last_message(lun: &Lun, task_id: i64) -> String {
    lun.logs_for("task", task_id).unwrap().remove(0).message
}

#[test]
fn default_messages_say_what_changed() {
    let (lun, root) = board("messages");
    let task = new_task(&lun);
    let key = &task.task_key;

    lun.update_task(
        task.id,
        TaskUpdateSpec {
            notes: Some("hello".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(last_message(&lun, task.id), format!("edit {key}; update note"));

    lun.update_task(
        task.id,
        TaskUpdateSpec {
            status: Some("review".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(last_message(&lun, task.id), format!("edit {key}; update status to Review"));

    lun.update_task(
        task.id,
        TaskUpdateSpec {
            status: Some("waiting-on".into()),
            priority: Some("high".into()),
            notes: Some("hello again".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        last_message(&lun, task.id),
        format!("edit {key}; update status to Waiting-On; update priority to high; update note")
    );

    // set_notes (TUI / --notes path) words it the same way.
    lun.set_notes(task.id, "third", None, None).unwrap();
    assert_eq!(last_message(&lun, task.id), format!("edit {key}; update note"));

    // An explicit message always wins.
    lun.set_notes(task.id, "fourth", Some("did: rewrote it"), None).unwrap();
    assert_eq!(last_message(&lun, task.id), "did: rewrote it");

    // The CLI prompt default is built from the same logic, before the edit.
    let current = lun.task_by_key(key).unwrap();
    let spec = TaskUpdateSpec {
        status: Some("closed".into()),
        ..Default::default()
    };
    assert_eq!(
        Lun::default_edit_message(&current, &spec),
        format!("edit {key}; update status to Closed")
    );
    assert_eq!(
        Lun::default_edit_message(&current, &TaskUpdateSpec::default()),
        format!("edit {key}")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reopening_an_upgraded_board_keeps_hashes_stable() {
    let (lun, root) = board("stable");
    let task = new_task(&lun);
    lun.set_notes(task.id, "a", None, None).unwrap();
    let before: Vec<String> = lun
        .logs_for("task", task.id)
        .unwrap()
        .into_iter()
        .map(|l| l.hash)
        .collect();
    drop(lun);
    let lun = Lun::init(&root).unwrap(); // re-running init is allowed anytime
    let after: Vec<String> = lun
        .logs_for("task", task.id)
        .unwrap()
        .into_iter()
        .map(|l| l.hash)
        .collect();
    assert_eq!(before, after);
    let _ = std::fs::remove_dir_all(&root);
}

/// A real v10 board has no `logs.hash`, and its note diffs do not know their
/// log entry. Rebuild that shape by hand, upgrade it, and check the backfill:
/// every entry hashed, and every diff linked to *its own* entry even when
/// several edits share one timestamp second.
#[test]
fn v10_board_upgrades_with_hashes_and_correctly_linked_diffs() {
    let (lun, root) = board("v10");
    new_task(&lun);
    drop(lun);

    let conn = rusqlite::Connection::open(root.join(".lun/lun.db")).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;
         DROP INDEX idx_logs_hash;
         ALTER TABLE logs DROP COLUMN hash;
         DROP TABLE note_diffs;
         CREATE TABLE note_diffs (
            id INTEGER PRIMARY KEY,
            task_id INTEGER NOT NULL REFERENCES tasks(id),
            version INTEGER NOT NULL,
            diff TEXT NOT NULL,
            old_text TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            user TEXT NOT NULL
         );
         DELETE FROM migrations WHERE version = 11;
         INSERT INTO migrations (version, applied_at) VALUES (10, '2026-01-01T00:00:00Z');
         -- (a) predates v10: a notes entry but no diff
         INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details)
           VALUES ('task', 1, '2026-01-01T00:00:00Z', 'me', 'UPDATE', 'a-pre-v10',
                   '{\"changes\": \"notes: empty -> 1 line\"}');
         -- (b) and (d) are the notes edits that have diffs, (c) is not a notes edit;
         -- (b) and (d) share a timestamp on purpose
         INSERT INTO logs (entity_type, entity_id, timestamp, user, action, message, details)
           VALUES ('task', 1, '2026-01-02T00:00:00Z', 'me', 'UPDATE', 'b-first-diff',
                   '{\"changes\": \"notes: 1 line -> 2 lines\"}'),
                  ('task', 1, '2026-01-02T00:00:00Z', 'me', 'COMMIT', 'c-not-notes', '{}'),
                  ('task', 1, '2026-01-02T00:00:00Z', 'me', 'UPDATE', 'd-second-diff',
                   '{\"changes\": \"notes: 2 lines -> 3 lines\"}');
         INSERT INTO note_diffs (task_id, version, diff, old_text, timestamp, user)
           VALUES (1, 1, '@@ b @@', 'one', '2026-01-02T00:00:00Z', 'me'),
                  (1, 2, '@@ d @@', 'one\ntwo', '2026-01-02T00:00:00Z', 'me');",
    )
    .unwrap();
    drop(conn);

    let lun = Lun::init(&root).unwrap();
    assert_eq!(lun.schema_version().unwrap(), lun::db::CURRENT_VERSION);

    let all = lun.logs_for("task", 1).unwrap();
    assert!(all.iter().all(|l| is_hash(&l.hash)), "every entry is hashed");
    let unique: HashSet<&str> = all.iter().map(|l| l.hash.as_str()).collect();
    assert_eq!(unique.len(), all.len(), "hashes are unique");

    let id_of = |msg: &str| all.iter().find(|l| l.message == msg).unwrap().id;
    let diffs = lun.get_note_diffs(1).unwrap();
    assert_eq!(diffs[0].log_id, Some(id_of("b-first-diff")));
    assert_eq!(diffs[1].log_id, Some(id_of("d-second-diff")));

    // ...and the hash of entry (d) hands back the notes as they were before it.
    let d_hash = &all.iter().find(|l| l.message == "d-second-diff").unwrap().hash;
    let entry = lun.log_by_hash(&d_hash[..7]).unwrap();
    assert_eq!(
        lun.note_diff_for_log(entry.id).unwrap().unwrap().old_text,
        "one\ntwo"
    );
    let _ = std::fs::remove_dir_all(&root);
}
