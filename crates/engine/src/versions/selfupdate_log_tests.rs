use super::record::Fact;
use super::selfupdate_log::{append, mark_recorded, unrecorded, DOC, KEEP};
use super::Tool;
use crate::sessions::testutil::TempDir;
use crate::store;

fn add(dir: &TempDir, n: i64, detail: &str) {
    append(dir.root(), n, Tool::Claude, "vps", "off", true, detail).unwrap();
}

#[test]
fn entries_keep_their_order_and_numbers() {
    let dir = TempDir::new("selflog-order");
    add(&dir, 1, "Self-update off in ~/.claude/settings.json");
    add(&dir, 2, "Self-update was already off");
    let found = unrecorded(dir.root());
    assert_eq!(found.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(found[0].at_ms, 1);
    assert_eq!(found[0].machine, "vps");
    assert_eq!(
        found[1].fact(),
        Fact::SelfUpdate {
            tool: Tool::Claude,
            machine: "vps".into(),
            action: "off".into(),
            ok: true,
            detail: "Self-update was already off".into(),
        }
    );
}

#[test]
fn only_the_last_two_hundred_are_kept() {
    let dir = TempDir::new("selflog-keep");
    for n in 0..(KEEP as i64 + 25) {
        add(&dir, n, "x");
    }
    let found = unrecorded(dir.root());
    assert_eq!(found.len(), KEEP);
    assert_eq!(found[0].seq, 26);
    assert_eq!(found[KEEP - 1].seq, KEEP as u64 + 25);
}

#[test]
fn recorded_entries_are_not_handed_out_again() {
    let dir = TempDir::new("selflog-recorded");
    add(&dir, 1, "a");
    add(&dir, 2, "b");
    mark_recorded(dir.root(), 1).unwrap();
    assert_eq!(unrecorded(dir.root()).len(), 1);
    mark_recorded(dir.root(), 0).unwrap();
    assert_eq!(unrecorded(dir.root()).len(), 1);
    add(&dir, 3, "c");
    assert_eq!(
        unrecorded(dir.root())
            .iter()
            .map(|e| e.seq)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    mark_recorded(dir.root(), 3).unwrap();
    assert!(unrecorded(dir.root()).is_empty());
}

#[test]
fn a_detail_is_one_short_line() {
    let dir = TempDir::new("selflog-cut");
    add(&dir, 1, &format!("first line\n{}", "z".repeat(900)));
    add(&dir, 2, &"y".repeat(900));
    let found = unrecorded(dir.root());
    assert_eq!(found[0].detail, "first line");
    assert_eq!(found[1].detail.len(), 300);
}

#[test]
fn a_broken_doc_starts_again() {
    let dir = TempDir::new("selflog-broken");
    store::put_doc(dir.root(), DOC, "not json").unwrap();
    assert!(unrecorded(dir.root()).is_empty());
    add(&dir, 1, "a");
    assert_eq!(unrecorded(dir.root()).len(), 1);
}
