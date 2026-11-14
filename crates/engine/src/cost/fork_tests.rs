use super::summary::Window;
use super::tracker::{Root, Tracker, CODEX};
use crate::catalog::Catalog;
use crate::sessions::testutil::TempDir;
use crate::transcript::codex::testutil::{FORKED, OLD_FORK, PARENT, PLAIN};
use std::sync::Arc;

const NOW: f64 = 1_800_000_000.0;

fn utc(_: f64) -> i64 {
    0
}

fn tracker(tmp: &TempDir) -> Tracker {
    Tracker::new(
        vec![Root {
            dir: tmp.path("codex"),
            channel: CODEX,
        }],
        None,
        None,
        Arc::new(Catalog::built_in()),
    )
    .with_zone(utc)
}

fn codex_tokens(t: &mut Tracker) -> f64 {
    t.refresh(NOW);
    t.ranged(NOW).overall[&Window::All]
        .channel_tokens
        .get("Codex")
        .copied()
        .unwrap_or(0.0)
}

#[test]
fn a_forked_block_is_not_counted_twice() {
    let tmp = TempDir::new("cost-fork");
    PARENT.write_in(&tmp, "codex");
    FORKED.write_in(&tmp, "codex");
    let mut t = tracker(&tmp);
    assert_eq!(
        codex_tokens(&mut t),
        (900.0 + 100.0) + (250.0 + 50.0),
        "the parent's own, plus what the child spent after its fork"
    );
}

#[test]
fn a_child_without_the_marker_counts_the_same() {
    let tmp = TempDir::new("cost-fork-old");
    PARENT.write_in(&tmp, "codex");
    OLD_FORK.write_in(&tmp, "codex");
    let mut t = tracker(&tmp);
    assert_eq!(codex_tokens(&mut t), 1000.0 + 300.0);
}

#[test]
fn a_child_with_no_forked_block_counts_in_full() {
    let tmp = TempDir::new("cost-plain");
    PARENT.write_in(&tmp, "codex");
    PLAIN.write_in(&tmp, "codex");
    let mut t = tracker(&tmp);
    assert_eq!(codex_tokens(&mut t), 1000.0 + 200.0);
}

#[test]
fn a_child_keeps_its_account_after_the_skip() {
    let tmp = TempDir::new("cost-fork-account");
    FORKED.write_in(&tmp, "codex");
    let mut t = tracker(&tmp);
    t.refresh(NOW);
    let r = t.ranged(NOW);
    assert!(r.by_account[&Window::All].contains_key("user-1:acct-1"));
}

#[test]
fn a_child_written_in_steps_counts_only_its_own_tokens() {
    let tmp = TempDir::new("cost-fork-steps");
    PARENT.write_in(&tmp, "codex");
    let path = FORKED.write_in(&tmp, "codex");
    std::fs::write(&path, FORKED.up_to_line(8)).expect("the fork alone");
    let mut t = tracker(&tmp);
    assert_eq!(
        codex_tokens(&mut t),
        1000.0,
        "nothing of the fork before the child's own turn exists"
    );
    std::fs::write(&path, FORKED.text).expect("the whole file");
    assert_eq!(codex_tokens(&mut t), 1000.0 + 300.0);
}
