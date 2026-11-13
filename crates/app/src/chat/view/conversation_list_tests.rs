use gpui::{px, ListAlignment, ListOffset, ListState};
use notchling_engine::transcript::model::{ChatItem, ItemKind, ToolCall, ToolKind};

use super::*;
use crate::chat::data::Tool;
use crate::chat::log::diff;
use crate::chat::rows::{rows, with_outgoing};
use crate::chat::view::conversation_view::SessionShown;

fn item(id: &str, text: &str) -> ChatItem {
    ChatItem {
        id: id.into(),
        kind: ItemKind::Assistant { text: text.into() },
        date: None,
    }
}

fn conv(range: Range<usize>) -> Arc<Conversation> {
    Arc::new(Conversation {
        items: range.map(|i| item(&format!("i{i}"), "text")).collect(),
        ..Conversation::default()
    })
}

fn input_for(session: &str, shown: Arc<Conversation>) -> ConversationInput {
    ConversationInput {
        shown,
        subagent: None,
        session: Some(SessionShown {
            id: session.into(),
            tool: Tool::Claude,
            cwd: "/work".into(),
            working: false,
        }),
        held_working: false,
        outgoing: Vec::new(),
        loading: false,
        empty_note: None,
        subagents: Vec::new(),
        tasks_inset: false,
        selected: Some(session.into()),
        chat: gpui::WeakEntity::new_invalid(),
    }
}

fn input(shown: Arc<Conversation>) -> ConversationInput {
    input_for("s1", shown)
}

fn list() -> ListRows {
    ListRows::new(ListState::new(0, ListAlignment::Bottom, px(OVERDRAW)))
}

fn pending(old: &Arc<Conversation>, new: &Arc<Conversation>) -> Pending {
    (old.clone(), new.clone(), diff(old, new))
}

fn top(rows: &ListRows) -> (usize, f32) {
    let at = rows.state.logical_scroll_top();
    (at.item_ix, f32::from(at.offset_in_item))
}

#[test]
fn an_append_splices_one_row_before_the_bottom_spacer() {
    let mut rows = list();
    let c0 = conv(0..3);
    rows.sync(&input(c0.clone()), None, false);
    assert_eq!(rows.state.item_count(), 5);
    let c1 = conv(0..4);
    let change = pending(&c0, &c1);
    rows.sync(&input(c1), Some(&change), false);
    assert_eq!(rows.state.item_count(), 6);
    assert_eq!(rows.spliced(), vec![(4..4, 1)]);
    assert_eq!(rows.keys().last().map(|k| k.as_ref()), Some("i3"));
}

#[test]
fn a_front_trim_removes_rows_from_the_top() {
    let mut rows = list();
    let c0 = conv(0..5);
    rows.sync(&input(c0.clone()), None, false);
    rows.state.scroll_to(ListOffset {
        item_ix: 4,
        offset_in_item: px(10.0),
    });
    let c1 = conv(2..5);
    let change = pending(&c0, &c1);
    rows.sync(&input(c1), Some(&change), false);
    assert_eq!(rows.state.item_count(), 5);
    assert_eq!(rows.spliced(), vec![(1..3, 0)]);
    assert_eq!(rows.keys().first().map(|k| k.as_ref()), Some("i2"));
    assert_eq!(top(&rows), (2, 10.0));
}

#[test]
fn another_session_resets_to_the_end() {
    let mut rows = list();
    let c0 = conv(0..6);
    rows.sync(&input(c0), None, false);
    rows.state.scroll_to(ListOffset {
        item_ix: 2,
        offset_in_item: px(5.0),
    });
    assert!(!rows.at_bottom() || rows.state.logical_scroll_top().item_ix < rows.state.item_count());
    let other = conv(10..14);
    let done = rows.sync(&input_for("s2", other), None, false);
    assert!(done.keys);
    assert_eq!(rows.state.item_count(), 6);
    assert_eq!(top(&rows).0, rows.state.item_count(), "following the end");
    assert!(rows.at_bottom());
    assert_eq!(rows.keys().len(), 4);
}

#[test]
fn the_tail_rows_follow_the_queue() {
    let mut rows = list();
    let c0 = conv(0..3);
    rows.sync(&input(c0.clone()), None, false);
    let mut typed = input(c0.clone());
    typed.outgoing = vec!["hello".into()];
    rows.sync(&typed, None, false);
    assert_eq!(rows.state.item_count(), 6);
    assert_eq!(rows.spliced(), vec![(4..4, 1)]);
    assert_eq!(rows.keys().last().map(|k| k.as_ref()), Some("sending-0"));
    let mut queued = Conversation::clone(&c0);
    queued.queued = vec!["hello".into()];
    let queued = Arc::new(queued);
    let change = pending(&c0, &queued);
    rows.sync(&input(queued.clone()), Some(&change), false);
    assert_eq!(rows.state.item_count(), 6);
    assert_eq!(rows.keys().last().map(|k| k.as_ref()), Some("queued-0"));
    let mut sent = Conversation::clone(&queued);
    sent.queued.clear();
    sent.items.push(item("i3", "hello"));
    let sent = Arc::new(sent);
    let change = pending(&queued, &sent);
    rows.sync(&input(sent), Some(&change), false);
    assert_eq!(rows.state.item_count(), 6);
    assert_eq!(rows.keys().last().map(|k| k.as_ref()), Some("i3"));
}

#[test]
fn the_keys_are_the_ones_the_rows_were_selected_by() {
    let mut shown = Conversation::clone(&conv(0..3));
    shown.queued = vec!["a".into(), "b".into()];
    let outgoing = vec!["c".to_string()];
    let rows = with_outgoing(rows(&shown), &outgoing);
    let drawn: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| crate::chat::view::row_select::key_of(&shown.items, i, row))
        .collect();
    assert_eq!(keys_of(&shown, &outgoing), drawn);
    for (i, row) in rows.iter().enumerate() {
        let mut input = input(Arc::new(shown.clone()));
        input.outgoing = outgoing.clone();
        assert_eq!(row_at(&input, i).as_ref(), Some(row), "row {i}");
    }
}

#[test]
fn the_live_row_is_remeasured_when_it_moves() {
    let tools = ChatItem {
        id: "t1".into(),
        kind: ItemKind::Tools(vec![ToolCall::new(
            "c1",
            "Bash",
            ToolKind::Shell,
            "List".into(),
            "ls".into(),
        )]),
        date: None,
    };
    let shown = Arc::new(Conversation {
        items: vec![item("i0", "text"), tools],
        ..Conversation::default()
    });
    let mut working = input(shown.clone());
    working.session.as_mut().unwrap().working = true;
    let mut rows = list();
    rows.sync(&working, None, false);
    rows.spliced.borrow_mut().clear();
    let idle = input(shown);
    rows.sync(&idle, None, false);
    assert_eq!(rows.spliced(), vec![(2..3, 1)]);
}

#[test]
fn the_tasks_bar_remeasures_the_top_spacer() {
    let mut rows = list();
    let shown = conv(0..3);
    rows.sync(&input(shown.clone()), None, false);
    rows.spliced.borrow_mut().clear();
    let mut inset = input(shown);
    inset.tasks_inset = true;
    rows.sync(&inset, None, false);
    assert_eq!(rows.spliced(), vec![(0..1, 1)]);
}

#[test]
fn at_bottom_ignores_unmeasured_rows() {
    let state = ListState::new(10, ListAlignment::Bottom, px(OVERDRAW));
    state.scroll_to(ListOffset {
        item_ix: 10,
        offset_in_item: px(0.0),
    });
    let rows = ListRows::new(state);
    assert!(rows.at_bottom());
    assert!(list().at_bottom());
}

#[test]
fn a_splice_at_the_scroll_top_keeps_its_offset() {
    let mut rows = list();
    let c0 = conv(0..5);
    rows.sync(&input(c0.clone()), None, false);
    rows.state.scroll_to(ListOffset {
        item_ix: 3,
        offset_in_item: px(14.0),
    });
    let mut changed = Conversation::clone(&c0);
    changed.items[2] = item("i2", "a longer text");
    let c1 = Arc::new(changed);
    let change = pending(&c0, &c1);
    rows.sync(&input(c1), Some(&change), false);
    assert_eq!(rows.spliced(), vec![(3..4, 1)]);
    assert_eq!(top(&rows), (3, 14.0));
    rows.state.splice(3..4, 1);
    assert_eq!(top(&rows), (3, 0.0));
}

#[test]
fn a_reader_scrolled_up_is_not_moved_by_an_append_and_one_at_the_end_follows() {
    let mut rows = list();
    let c0 = conv(0..5);
    rows.sync(&input(c0.clone()), None, false);
    rows.state.scroll_to(ListOffset {
        item_ix: 2,
        offset_in_item: px(7.0),
    });
    let c1 = conv(0..6);
    let change = pending(&c0, &c1);
    rows.sync(&input(c1.clone()), Some(&change), false);
    assert_eq!(top(&rows), (2, 7.0));
    let c2 = conv(0..7);
    let change = pending(&c1, &c2);
    rows.sync(&input(c2.clone()), Some(&change), true);
    assert!(rows.at_bottom());
    assert_eq!(top(&rows).0, rows.state.item_count());
    let c3 = conv(0..8);
    let change = pending(&c2, &c3);
    rows.sync(&input(c3), Some(&change), false);
    assert_eq!(top(&rows).0, rows.state.item_count());
}

#[test]
fn a_reset_in_the_same_session_keeps_the_row_being_read() {
    let mut rows = list();
    let c0 = conv(0..8);
    rows.sync(&input(c0), None, false);
    rows.state.scroll_to(ListOffset {
        item_ix: 4,
        offset_in_item: px(9.0),
    });
    let c1 = conv(0..9);
    let done = rows.sync(&input(c1), None, false);
    assert!(done.keys);
    assert_eq!(top(&rows), (4, 9.0));
}

#[test]
fn a_drag_scroll_moves_a_list_that_follows_its_end_at_once() {
    assert_eq!(scroll_target(900.0, 500.0, -10.0), 490.0);
    assert_eq!(scroll_target(500.0, 500.0, -10.0), 490.0);
    assert_eq!(scroll_target(200.0, 500.0, 40.0), 240.0);
    assert_eq!(scroll_target(495.0, 500.0, 40.0), 500.0);
    assert_eq!(scroll_target(5.0, 500.0, -40.0), 0.0);
    assert_eq!(scroll_target(0.0, 0.0, 30.0), 0.0);
}

#[test]
fn a_draw_with_nothing_new_leaves_a_reader_near_the_end_where_they_are() {
    assert!(goes_to_end(true, false, true));
    assert!(!goes_to_end(true, false, false));
    assert!(!goes_to_end(false, false, true));
    assert!(goes_to_end(false, true, false));
    assert!(goes_to_end(true, true, true));
}
