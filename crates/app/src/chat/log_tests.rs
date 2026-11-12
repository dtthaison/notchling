//! `chat::log::diff`: what changed between two loads of a conversation, by item.

use std::sync::Arc;

use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};

use super::log::{diff, Change, Splice, RESET_ABOVE};

fn item(n: usize, text: &str) -> ChatItem {
    ChatItem {
        id: format!("i{n}"),
        kind: ItemKind::Assistant { text: text.into() },
        date: None,
    }
}

fn conversation(items: &[(usize, &str)]) -> Arc<Conversation> {
    Arc::new(Conversation {
        items: items.iter().map(|(n, t)| item(*n, t)).collect(),
        ..Conversation::default()
    })
}

/// Items `from..to` with their own number as text.
fn run(from: usize, to: usize) -> Arc<Conversation> {
    Arc::new(Conversation {
        items: (from..to).map(|n| item(n, &format!("t{n}"))).collect(),
        ..Conversation::default()
    })
}

fn splice(old: std::ops::Range<usize>, new: std::ops::Range<usize>) -> Splice {
    Splice { old, new }
}

#[test]
fn the_same_arc_is_same() {
    let a = run(0, 5);
    assert_eq!(diff(&a, &a.clone()), Change::Same);
}

#[test]
fn equal_copies_are_same() {
    let a = run(0, 5);
    let b = Arc::new((*a).clone());
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(diff(&a, &b), Change::Same);
}

#[test]
fn an_appended_item_is_one_splice_at_the_end() {
    assert_eq!(
        diff(&run(0, 3), &run(0, 4)),
        Change::Items {
            splices: vec![splice(3..3, 3..4)],
            other: false
        }
    );
}

#[test]
fn a_changed_last_item_is_replaced_in_place() {
    // A tools group gained a step: same id, different content.
    let old = conversation(&[(0, "a"), (1, "b"), (2, "c")]);
    let new = conversation(&[(0, "a"), (1, "b"), (2, "c and more")]);
    assert_eq!(
        diff(&old, &new),
        Change::Items {
            splices: vec![splice(2..3, 2..3)],
            other: false
        }
    );
}

#[test]
fn neighbouring_changed_items_are_one_splice() {
    let old = conversation(&[(0, "a"), (1, "b"), (2, "c"), (3, "d")]);
    let new = conversation(&[(0, "a"), (1, "B"), (2, "C"), (3, "d")]);
    assert_eq!(
        diff(&old, &new),
        Change::Items {
            splices: vec![splice(1..3, 1..3)],
            other: false
        }
    );
}

#[test]
fn a_front_trim_and_an_append_are_two_splices() {
    // The engine keeps the newest 500 items: two fell off the front, one was added.
    assert_eq!(
        diff(&run(0, 5), &run(2, 6)),
        Change::Items {
            splices: vec![splice(0..2, 0..0), splice(5..5, 3..4)],
            other: false
        }
    );
}

#[test]
fn renumbered_ids_reset() {
    // A fresh cursor numbers the lines again: the first new id isn't in the old items.
    let old = conversation(&[(10, "a"), (11, "b")]);
    let new = conversation(&[(0, "a"), (1, "b")]);
    assert_eq!(diff(&old, &new), Change::Reset);
    // Or it is, but what follows doesn't line up.
    let old = conversation(&[(0, "a"), (1, "b"), (2, "c")]);
    let new = conversation(&[(0, "a"), (5, "b"), (6, "c")]);
    assert_eq!(diff(&old, &new), Change::Reset);
}

#[test]
fn an_empty_side_resets() {
    let empty = Arc::new(Conversation::default());
    assert_eq!(diff(&empty, &run(0, 2)), Change::Reset);
    assert_eq!(diff(&run(0, 2), &empty), Change::Reset);
}

#[test]
fn more_than_reset_above_changes_resets() {
    // Room for RESET_ABOVE + 1 changed items with an unchanged one between each, so none merge.
    let total = 2 * (RESET_ABOVE + 1) + 2;
    let old = Arc::new(Conversation {
        items: (0..total).map(|n| item(n, "old")).collect(),
        ..Conversation::default()
    });
    let changed = |count: usize| {
        Arc::new(Conversation {
            items: (0..total)
                .map(|n| {
                    item(
                        n,
                        if n % 2 == 0 && n / 2 < count {
                            "new"
                        } else {
                            "old"
                        },
                    )
                })
                .collect(),
            ..Conversation::default()
        })
    };
    assert!(matches!(
        diff(&old, &changed(RESET_ABOVE)),
        Change::Items { .. }
    ));
    assert_eq!(diff(&old, &changed(RESET_ABOVE + 1)), Change::Reset);
}

#[test]
fn only_the_queue_changed_is_items_with_no_splices_and_other() {
    let old = run(0, 3);
    let mut next = (*old).clone();
    next.queued.push("next message".into());
    assert_eq!(
        diff(&old, &Arc::new(next)),
        Change::Items {
            splices: Vec::new(),
            other: true
        }
    );
}
