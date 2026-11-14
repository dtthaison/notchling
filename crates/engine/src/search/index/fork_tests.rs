use super::*;
use crate::sessions::testutil::TempDir;
use crate::transcript::codex::testutil::{Fixture, FORKED, OLD_FORK, PARENT, PLAIN};
use std::fs::OpenOptions;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub(super) fn place(dir: &TempDir, fixture: &Fixture, text: &str) -> std::path::PathBuf {
    let c = crate::pages::clock::civil_at(now(), 0);
    let rel = format!(
        "codex/sessions/{:04}/{:02}/{:02}/{}",
        c.year, c.month, c.day, fixture.name
    );
    let path = dir.write(&rel, text);
    let file = OpenOptions::new().write(true).open(&path).expect("open");
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64(now() - 60.0))
        .expect("age it");
    path
}

pub(super) fn roots(dir: &TempDir) -> IndexRoots {
    IndexRoots {
        claude: Vec::new(),
        codex_sessions: Some(dir.path("codex/sessions")),
        titled: Vec::new(),
    }
}

fn keys(index: &SessionIndex, word: &str) -> Vec<String> {
    let mut keys: Vec<String> = index
        .search(&[word], now(), 30)
        .into_iter()
        .map(|h| h.meta.key)
        .collect();
    keys.sort();
    keys
}

#[test]
fn a_childs_text_has_no_parent_words() {
    let dir = TempDir::new("idx-fork-words");
    for fixture in [&PARENT, &FORKED] {
        place(&dir, fixture, fixture.text);
    }
    let index = SessionIndex::new(None);
    assert_eq!(index.refresh(&roots(&dir), now()).read, 2);
    assert_eq!(
        keys(&index, "zebrafish"),
        [format!("codex:{}", PARENT.thread)],
        "the parent's words are the parent's alone"
    );
    assert_eq!(
        keys(&index, "tusks"),
        [format!("codex:{}", FORKED.thread)],
        "the child's own words are the child's"
    );
}

#[test]
fn a_child_without_the_marker_has_no_parent_words_either() {
    let dir = TempDir::new("idx-fork-old");
    for fixture in [&PARENT, &OLD_FORK] {
        place(&dir, fixture, fixture.text);
    }
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    assert_eq!(
        keys(&index, "zebrafish"),
        [format!("codex:{}", PARENT.thread)]
    );
    assert_eq!(
        keys(&index, "tusks"),
        [format!("codex:{}", OLD_FORK.thread)]
    );
}

#[test]
fn a_child_with_no_forked_block_is_indexed_whole() {
    let dir = TempDir::new("idx-fork-plain");
    place(&dir, &PLAIN, PLAIN.text);
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    assert_eq!(
        keys(&index, "plankton"),
        [format!("codex:{}", PLAIN.thread)]
    );
}

#[test]
fn a_childs_title_is_its_own_task() {
    let dir = TempDir::new("idx-fork-title");
    place(&dir, &FORKED, FORKED.text);
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    let hit = &index.search(&["tusks"], now(), 5)[0];
    assert_eq!(hit.meta.title, "Count the narwhal tusks");
    assert_eq!(hit.meta.folder, "/work/app");
}
