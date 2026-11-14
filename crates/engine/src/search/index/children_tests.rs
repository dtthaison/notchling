use super::fork_tests::{place, roots};
use super::*;
use crate::sessions::testutil::TempDir;
use crate::transcript::codex::testutil::{FORKED, OLD_FORK, PARENT, PLAIN};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[test]
fn a_child_is_indexed_under_its_parent() {
    let dir = TempDir::new("idx-children");
    for fixture in [&PARENT, &FORKED, &PLAIN, &OLD_FORK] {
        place(&dir, fixture, fixture.text);
    }
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    let parent_row = format!("codex:{}", PARENT.thread);
    let parents: Vec<(String, Option<String>)> = {
        let mut rows: Vec<_> = ["zebrafish", "tusks", "plankton"]
            .iter()
            .flat_map(|word| index.search(&[*word], now(), 30))
            .map(|h| (h.meta.key, h.meta.parent))
            .collect();
        rows.sort();
        rows.dedup();
        rows
    };
    let parent_of = |thread: &str| {
        parents
            .iter()
            .find(|(key, _)| key == &format!("codex:{thread}"))
            .map(|(_, parent)| parent.clone())
    };
    assert_eq!(parent_of(PARENT.thread), Some(None));
    assert_eq!(parent_of(FORKED.thread), Some(Some(parent_row.clone())));
    assert_eq!(parent_of(OLD_FORK.thread), Some(Some(parent_row.clone())));
    assert_eq!(parent_of(PLAIN.thread), Some(Some(parent_row)));
}

#[test]
fn a_child_of_an_unknown_parent_has_nowhere_to_open_and_is_not_indexed() {
    let dir = TempDir::new("idx-orphan");
    let orphan = [
        r#"{"ordinal":0,"type":"session_meta","payload":{"id":"lost","cwd":"/work/app","thread_source":"subagent"}}"#,
        r#"{"ordinal":1,"type":"event_msg","payload":{"type":"task_started"}}"#,
        r#"{"ordinal":2,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Review the plankton notes"}]}}"#,
    ]
    .join("\n")
        + "\n";
    place(&dir, &PLAIN, &orphan);
    let index = SessionIndex::new(None);
    let stats = index.refresh(&roots(&dir), now());
    assert_eq!(stats.read, 0);
    assert!(index.is_empty());
}

#[test]
fn old_rows_without_a_parent_still_read() {
    let stored = r#"{"key":"codex:a","toolKey":"codex","sessionID":"a","title":"t","folder":"/w","path":"/p","configDir":null,"modified":1.5,"size":3}"#;
    let meta: SessionMeta = serde_json::from_str(stored).expect("an old row");
    assert_eq!(meta.parent, None);
    let again = serde_json::to_string(&meta).expect("write");
    let back: SessionMeta = serde_json::from_str(&again).expect("read back");
    assert_eq!(back, meta);
}

#[test]
fn a_parent_found_through_the_database_keeps_the_link() {
    let dir = TempDir::new("idx-children-db");
    for fixture in [&PARENT, &FORKED] {
        place(&dir, fixture, fixture.text);
    }
    let db = dir.path("search.db");
    {
        let index = SessionIndex::new(Some(db.clone()));
        index.refresh(&roots(&dir), now());
    }
    let reopened = SessionIndex::new(Some(db));
    reopened.load_cache();
    let hit = reopened
        .search(&["tusks"], now(), 5)
        .into_iter()
        .next()
        .expect("a hit");
    assert_eq!(hit.meta.parent, Some(format!("codex:{}", PARENT.thread)));
}

#[test]
fn the_newest_list_has_sessions_and_no_children() {
    let dir = TempDir::new("idx-children-latest");
    for fixture in [&PARENT, &FORKED, &PLAIN] {
        place(&dir, fixture, fixture.text);
    }
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    let keys: Vec<String> = index
        .search::<&str>(&[], now(), 30)
        .into_iter()
        .map(|hit| hit.meta.key)
        .collect();
    assert_eq!(keys, [format!("codex:{}", PARENT.thread)]);
}
