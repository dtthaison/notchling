//! The conversation's selection as positions: no window, no layout.

use std::collections::HashMap;
use std::path::Path;

use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::{ChatItem, ItemKind};

use super::*;
use crate::chat::markdown_cache::Cache;
use crate::chat::rows::Row;
use crate::chat::selection::copied as old_copy;
use crate::chat::view::row_blocks::rows_blocks;

fn key(text: &str) -> SharedString {
    SharedString::from(text.to_owned())
}

fn spot(row: &str, block: u16, byte: usize) -> Spot {
    Spot {
        row: key(row),
        block,
        byte,
    }
}

fn rows(keys: &[&str]) -> HashMap<SharedString, usize> {
    keys.iter().enumerate().map(|(i, k)| (key(k), i)).collect()
}

fn blocks(list: &[(&str, &[&str])]) -> Vec<(SharedString, Vec<String>)> {
    list.iter()
        .map(|(row, texts)| (key(row), texts.iter().map(|t| (*t).to_owned()).collect()))
        .collect()
}

fn selection(anchor: Spot, focus: Spot) -> Selection {
    Selection {
        anchor: Some(anchor),
        focus: Some(focus),
        ..Selection::default()
    }
}

fn item(id: &str) -> ChatItem {
    ChatItem {
        id: id.into(),
        kind: ItemKind::user("hi"),
        date: None,
    }
}

fn note(row: &str, block: u16, text: &str) -> Note {
    Note {
        row: key(row),
        block,
        text: text.to_owned(),
        layout: TextLayout::default(),
    }
}

#[test]
fn ends_order_by_row_then_block_then_byte() {
    let on_show = rows(&["a", "b", "c"]);
    // The row decides first, whatever the block or byte.
    let (start, end) = selection(spot("c", 0, 0), spot("a", 5, 99))
        .ends(&on_show)
        .expect("both rows are on show");
    assert_eq!(
        (start.order >> 16, start.index),
        (0, 99),
        "row a comes first"
    );
    assert_eq!((end.order >> 16, end.index), (2, 0));
    // Within a row, the block, then the byte.
    let (start, end) = selection(spot("b", 3, 1), spot("b", 1, 40))
        .ends(&on_show)
        .expect("on show");
    assert_eq!((start.order & 0xffff, start.index), (1, 40));
    assert_eq!((end.order & 0xffff, end.index), (3, 1));
    let (start, end) = selection(spot("b", 2, 9), spot("b", 2, 4))
        .ends(&on_show)
        .expect("on show");
    assert_eq!((start.index, end.index), (4, 9));
}

#[test]
fn copy_takes_rows_that_are_not_drawn() {
    // Nothing is drawn here at all: the copy reads the model's blocks.
    let model = blocks(&[
        ("u1", &["first message"]),
        ("a1", &["a heading", "the middle row's paragraph"]),
        ("u2", &["last message"]),
    ]);
    let sel = selection(spot("u1", 0, 6), spot("u2", 0, 4));
    assert_eq!(
        copied(&sel, &model).as_deref(),
        Some("message\na heading\nthe middle row's paragraph\nlast")
    );
}

#[test]
fn a_selection_on_a_trimmed_row_is_dropped() {
    let model = blocks(&[("a", &["one"]), ("b", &["two"])]);
    let sel = selection(spot("gone", 0, 0), spot("b", 0, 3));
    assert_eq!(copied(&sel, &model), None, "its anchor row is not on show");
    let sel = selection(spot("a", 0, 0), spot("gone", 0, 3));
    assert_eq!(copied(&sel, &model), None, "nor is its focus row");

    // The frame lets it go for good when it sees the rows without it.
    let mut frame = SelFrame {
        rows: rows(&["a", "b"]),
        ..SelFrame::default()
    };
    frame.selection = selection(spot("a", 0, 0), spot("b", 0, 3));
    frame.prune();
    assert!(frame.selection.anchor.is_some(), "both rows are there");
    frame.rows = rows(&["b"]);
    frame.prune();
    assert!(frame.selection.anchor.is_none() && frame.selection.focus.is_none());
}

#[test]
fn a_drag_backwards_copies_the_same_text() {
    let model = blocks(&[
        ("a", &["alpha beta"]),
        ("b", &["gamma delta", "epsilon"]),
        ("c", &["zeta"]),
    ]);
    let forward = copied(&selection(spot("a", 0, 6), spot("c", 0, 2)), &model);
    let backward = copied(&selection(spot("c", 0, 2), spot("a", 0, 6)), &model);
    assert_eq!(forward.as_deref(), Some("beta\ngamma delta\nepsilon\nze"));
    assert_eq!(forward, backward);
}

#[test]
fn queued_rows_have_distinct_keys() {
    let items = [item("i1"), item("i2")];
    let queued = Row::User {
        text: "waiting".into(),
        images: 0,
        queued: true,
        sending: false,
    };
    let sending = Row::User {
        text: "going".into(),
        images: 0,
        queued: true,
        sending: true,
    };
    let keys = [
        key_of(&items, 0, &queued),
        key_of(&items, 1, &queued),
        key_of(&items, 2, &queued),
        key_of(&items, 3, &queued),
        key_of(&items, 4, &sending),
        key_of(&items, 5, &sending),
    ];
    assert_eq!(keys[0].as_ref(), "i1", "an item is its id");
    assert_eq!(keys[2].as_ref(), "queued-0");
    assert_eq!(keys[4].as_ref(), "sending-2");
    let distinct: std::collections::HashSet<_> = keys.iter().collect();
    assert_eq!(distinct.len(), keys.len(), "{keys:?}");
}

#[test]
fn the_file_panel_keeps_its_own_selection() {
    // The conversation has none: ⌘C copies the file panel's.
    assert_eq!(
        clipboard_text(None, || Some("from the file".into())).as_deref(),
        Some("from the file")
    );
    // The conversation has one: it wins, and the file panel isn't even asked.
    assert_eq!(
        clipboard_text(Some("from the chat".into()), || unreachable!("not asked")).as_deref(),
        Some("from the chat")
    );
    assert_eq!(clipboard_text(None, || None), None);
}

#[test]
fn a_double_click_takes_the_word_and_a_triple_click_the_text() {
    let mut frame = SelFrame {
        rows: rows(&["a"]),
        ..SelFrame::default()
    };
    frame.note_drawn(note("a", 0, "Make ONE size_chart, please"));
    frame.begin(spot("a", 0, 6), 2);
    assert_eq!(frame.selection.unit, Unit::Word);
    assert_eq!(frame.selection.anchor, Some(spot("a", 0, 5)));
    assert_eq!(frame.selection.focus, Some(spot("a", 0, 8)));
    let model = blocks(&[("a", &["Make ONE size_chart, please"])]);
    assert_eq!(copied(&frame.selection, &model).as_deref(), Some("ONE"));

    frame.begin(spot("a", 0, 6), 3);
    assert_eq!(frame.selection.unit, Unit::Block);
    assert_eq!(
        copied(&frame.selection, &model).as_deref(),
        Some("Make ONE size_chart, please")
    );

    // A single click selects nothing yet.
    frame.begin(spot("a", 0, 6), 1);
    assert_eq!(copied(&frame.selection, &model), None);
}

#[test]
fn a_drag_after_a_double_click_widens_to_whole_words() {
    let mut frame = SelFrame {
        rows: rows(&["a", "b"]),
        ..SelFrame::default()
    };
    frame.note_drawn(note("a", 0, "one two three"));
    frame.note_drawn(note("b", 0, "four five six"));
    frame.begin(spot("a", 0, 5), 2); // "two"
    assert!(frame.drag_to(spot("b", 0, 6)), "the focus moved");
    let model = blocks(&[("a", &["one two three"]), ("b", &["four five six"])]);
    assert_eq!(
        copied(&frame.selection, &model).as_deref(),
        Some("two three\nfour five"),
        "the end word is taken whole"
    );
    // Dragging back above the anchor (the start of the word that was double-clicked) widens the
    // other way, as the draw-order selection always did.
    assert!(frame.drag_to(spot("a", 0, 1)));
    assert_eq!(copied(&frame.selection, &model).as_deref(), Some("one "));
    assert!(!frame.drag_to(spot("a", 0, 2)), "same word, same end");
}

#[test]
fn a_selection_survives_a_reload_that_appends() {
    let before = blocks(&[("a", &["alpha beta"]), ("b", &["gamma"])]);
    let sel = selection(spot("a", 0, 6), spot("b", 0, 3));
    assert_eq!(copied(&sel, &before).as_deref(), Some("beta\ngam"));
    // Two rows come in at the end, and one at the front (the places shift).
    let after = blocks(&[
        ("new-front", &["x"]),
        ("a", &["alpha beta"]),
        ("b", &["gamma"]),
        ("c", &["appended"]),
        ("d", &["more"]),
    ]);
    assert_eq!(copied(&sel, &after).as_deref(), Some("beta\ngam"));
}

#[test]
fn a_changed_message_clamps_the_selection_to_its_new_text() {
    let sel = selection(spot("a", 0, 6), spot("a", 0, 40));
    let long = blocks(&[("a", &["alpha beta gamma delta epsilon zeta eta"])]);
    assert_eq!(
        copied(&sel, &long).as_deref(),
        Some("beta gamma delta epsilon zeta eta")
    );
    // The message was edited and got shorter: the end is the end of what is left.
    let short = blocks(&[("a", &["alpha beta gamma"])]);
    assert_eq!(copied(&sel, &short).as_deref(), Some("beta gamma"));
    // A cut inside a character takes the whole character's start, never half of it.
    let sel = selection(spot("a", 0, 0), spot("a", 0, 3));
    assert_eq!(
        copied(&sel, &blocks(&[("a", &["Chào bạn"])])).as_deref(),
        Some("Ch")
    );
    // The block went (the message lost a paragraph): nothing is copied from it.
    let sel = selection(spot("a", 2, 0), spot("a", 2, 4));
    assert_eq!(copied(&sel, &long), None);
}

fn fixture_conversations() -> Vec<(String, notchling_engine::transcript::model::Conversation)> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut out = Vec::new();
    let mut paths: Vec<_> = Vec::new();
    for dir in [
        "chat",
        "chat-steps",
        "chat-tasks",
        "chat-permission",
        "notch",
    ] {
        let mut stack = vec![fixtures.join(dir)];
        while let Some(path) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if !p.to_string_lossy().contains("codex") {
                        stack.push(p);
                    }
                } else if p.extension().is_some_and(|e| e == "jsonl")
                    && p.to_string_lossy().contains("/claude/projects/")
                {
                    paths.push(p);
                }
            }
        }
    }
    paths.sort();
    for p in paths {
        let c = conversation::read(&p.to_string_lossy());
        if !c.items.is_empty() {
            out.push((p.display().to_string(), c));
        }
    }
    out
}

/// Every drag over the fixtures' conversations copies what the draw-order selection did: the
/// same blocks in the same order, with the same cuts.
#[test]
fn copies_the_same_text_as_the_draw_order_selection_on_every_fixture() {
    let conversations = fixture_conversations();
    assert!(conversations.len() >= 6, "{} fixtures", conversations.len());
    let mut checked = 0;
    let mut blocks_seen = 0;
    for (name, conversation) in conversations {
        let cache = std::cell::RefCell::new(Cache::default());
        // Everything open, so step details are blocks too, and two queued and one sent message
        // at the tail.
        let model = rows_blocks(
            &conversation,
            &["still going".to_owned()],
            None,
            "",
            &cache,
            &|_| true,
        );
        let flat: Vec<(usize, usize, &str)> = model
            .iter()
            .enumerate()
            .flat_map(|(r, (_, texts))| {
                texts
                    .iter()
                    .enumerate()
                    .map(move |(b, t)| (r, b, t.as_str()))
            })
            .collect();
        blocks_seen += flat.len();
        let texts: Vec<&str> = flat.iter().map(|(_, _, t)| *t).collect();
        let stride = (flat.len() / 40).max(1);
        let picks: Vec<usize> = (0..flat.len()).step_by(stride).collect();
        for &i in &picks {
            for &j in &picks {
                for (bi, bj) in [(0, 0), (0, usize::MAX), (3, 5), (usize::MAX, 2), (7, 1)] {
                    let (ai, aj) = (bi.min(texts[i].len()), bj.min(texts[j].len()));
                    let at = |k: usize, byte: usize| Spot {
                        row: model[flat[k].0].0.clone(),
                        block: flat[k].1 as u16,
                        byte,
                    };
                    let (start, end) = crate::chat::selection::ordered(
                        At {
                            order: i,
                            index: ai,
                        },
                        At {
                            order: j,
                            index: aj,
                        },
                    );
                    let old = old_copy(&texts, start, end);
                    let want = (!old.is_empty()).then_some(old);
                    let got = copied(&selection(at(i, ai), at(j, aj)), &model);
                    assert_eq!(
                        got, want,
                        "{name}: block {i} byte {ai} to block {j} byte {aj}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(blocks_seen > 100, "{blocks_seen} blocks");
    assert!(checked > 5000, "{checked} drags");
}
