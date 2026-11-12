//! What a [`Loader`] says about the conversation between two loads: the same `Arc` and `Same` when
//! nothing changed, the splice for an appended line, and no change for another session.

use std::sync::Arc;

use super::data::DataRoot;
use super::loader::Loader;
use super::log::{Change, Splice};
use crate::testutil::Scratch;

const FIX_JOB: &str = include_str!("../../fixtures/chat/claude/jobs/aa11bb22/state.json");
const FIX_LOG: &str =
    include_str!("../../fixtures/chat/claude/projects/-Users-me-work-notchling/fix-the-bug.jsonl");
const PORT_JOB: &str = include_str!("../../fixtures/chat/claude/jobs/cc33dd44/state.json");
const PORT_LOG: &str =
    include_str!("../../fixtures/chat/claude/projects/-Users-me-work-notchling/port-search.jsonl");
const LOG_PATH: &str = "claude/projects/-Users-me-work-notchling/fix-the-bug.jsonl";
const ONE_MORE: &str = concat!(
    r#"{"type":"user","timestamp":"2026-11-08T08:20:00.000Z","message":{"role":"user","content":"One more thing."}}"#,
    "\n"
);

/// Two background sessions, `fix-the-bug` and `port-search`, from the `chat` fixture's files.
fn world(s: &Scratch) -> Loader {
    s.write("claude/jobs/aa11bb22/state.json", FIX_JOB);
    s.write(LOG_PATH, FIX_LOG);
    s.write("claude/jobs/cc33dd44/state.json", PORT_JOB);
    s.write(
        "claude/projects/-Users-me-work-notchling/port-search.jsonl",
        PORT_LOG,
    );
    Loader::new(DataRoot::fixture(s.path()))
}

#[test]
fn an_unchanged_reload_is_same_and_the_same_arc() {
    let s = Scratch::new("loader-change-same");
    let mut loader = world(&s);
    let first = loader.load(Some("fix-the-bug"), 1_000);
    assert_eq!(first.selected.as_deref(), Some("fix-the-bug"));
    assert!(!first.conversation.items.is_empty());
    assert!(
        first.change.is_none(),
        "nothing to compare the first load with"
    );

    let second = loader.load(Some("fix-the-bug"), 2_000);
    assert!(Arc::ptr_eq(&first.conversation, &second.conversation));
    let (from, change) = second.change.expect("the same session is compared");
    assert!(Arc::ptr_eq(&from, &first.conversation));
    assert_eq!(change, Change::Same);
}

#[test]
fn an_appended_line_is_one_splice() {
    let s = Scratch::new("loader-change-append");
    let mut loader = world(&s);
    let first = loader.load(Some("fix-the-bug"), 1_000);
    let before = first.conversation.items.len();

    s.write(LOG_PATH, &format!("{FIX_LOG}{ONE_MORE}"));
    let second = loader.load(Some("fix-the-bug"), 2_000);
    assert!(!Arc::ptr_eq(&first.conversation, &second.conversation));
    assert_eq!(second.conversation.items.len(), before + 1);
    let (from, change) = second.change.expect("the same session is compared");
    assert!(Arc::ptr_eq(&from, &first.conversation));
    match change {
        Change::Items { splices, .. } => {
            assert_eq!(
                splices,
                vec![Splice {
                    old: before..before,
                    new: before..before + 1
                }]
            );
        }
        other => panic!("an appended line is a splice, not {other:?}"),
    }
}

#[test]
fn another_session_has_no_change() {
    let s = Scratch::new("loader-change-other");
    let mut loader = world(&s);
    loader.load(Some("fix-the-bug"), 1_000);
    let other = loader.load(Some("port-search"), 2_000);
    assert_eq!(other.selected.as_deref(), Some("port-search"));
    assert!(other.change.is_none(), "another session isn't compared");
    // And back: the loader remembers only the last one it loaded.
    let back = loader.load(Some("fix-the-bug"), 3_000);
    assert!(back.change.is_none());
}
