use crate::ask_testkit::{
    item_of, items, new_row, open, person, rig, rig_full, scratch, state_of, wait_as, ASKER, T0,
};
use crate::event_assert::{of, text};
use notchling_engine::ask::{AskState, AskStore, By};
use notchling_engine::inbox::{ActionId, Kind, State};
use notchling_local_api::ops;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const TOKEN: &str = "tok-7731-planted";

#[test]
fn settled_raises_one_item_and_keeps_its_id() {
    let r = rig();
    let id = open(&r);
    let row = r.store.get(&id).unwrap().unwrap();
    let inbox_id = row.inbox_id.clone().unwrap();
    assert!(inbox_id.starts_with(&format!("ask:{id}:")));
    let all = items(&r);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, inbox_id);
    assert_eq!(all[0].kind, Kind::Question);
    assert_eq!(all[0].state, State::Open);
    assert_eq!(all[0].detail, "Which one?");
    let ids: Vec<ActionId> = all[0].actions.iter().map(|a| a.id).collect();
    assert_eq!(
        ids,
        vec![
            ActionId::Choice1,
            ActionId::Choice2,
            ActionId::Open,
            ActionId::Cancel
        ]
    );
    r.ops.settled(&id);
    assert_eq!(items(&r).len(), 1);
    assert_eq!(
        r.store.get(&id).unwrap().unwrap().inbox_id.unwrap(),
        inbox_id
    );
}

#[test]
fn phone_choice_answers_it() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    let done = r
        .inbox
        .decide(&item.id, ActionId::Choice2, "phone")
        .unwrap();
    assert!(done.answered);
    r.ops.drain();
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!(row.state, AskState::Answered);
    assert_eq!(row.choice, Some(2));
    assert_eq!(row.answer.as_deref(), Some("b"));
    assert_eq!(row.by, Some(By::Phone));
    let item = item_of(&r, &id);
    assert_eq!(item.state, State::Answered);
    assert_eq!(item.answer, Some(ActionId::Choice2));
    assert_eq!(item.answered_by.as_deref(), Some("phone"));
    let reply = wait_as(&r, ASKER, &id, 500).unwrap();
    assert_eq!(reply["by"], json!("phone"));
}

#[test]
fn a_choice_from_the_mac_is_by_mac() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    r.inbox.decide(&item.id, ActionId::Choice1, "mac").unwrap();
    r.ops.drain();
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!(row.by, Some(By::Mac));
    assert_eq!(row.choice, Some(1));
}

#[test]
fn answer_in_chat_is_not_taken_by_the_inbox() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    let done = r.inbox.decide(&item.id, ActionId::Open, "phone").unwrap();
    assert!(!done.answered);
    r.ops.drain();
    assert_eq!(state_of(&r, &id), "open");
    assert_eq!(item_of(&r, &id).state, State::Open);
}

#[test]
fn chat_answer_resolves_the_item() {
    let r = rig();
    let id = open(&r);
    let text = format!("use {TOKEN}");
    let done = person(&r, ops::ASK_ANSWER, json!({"id": id, "text": text})).unwrap();
    assert_eq!(done["answered"], json!(true));
    let item = item_of(&r, &id);
    assert_eq!(item.state, State::Answered);
    assert_eq!(item.answer, None);
    assert_eq!(item.answered_by.as_deref(), Some("mac"));
    assert!(!serde_json::to_string(&item).unwrap().contains(TOKEN));
}

#[test]
fn a_chat_choice_resolves_with_its_action() {
    let r = rig();
    let id = open(&r);
    r.ops.answer(&id, None, Some(2), By::Main).unwrap();
    let item = item_of(&r, &id);
    assert_eq!(item.answer, Some(ActionId::Choice2));
    assert_eq!(item.answered_by.as_deref(), Some("main"));
}

#[test]
fn inbox_and_chat_race_one_wins() {
    let r = rig();
    let mut inbox_wins = 0;
    let mut chat_wins = 0;
    for round in 0..300 {
        let row = r
            .store
            .insert(
                &new_row(
                    crate::acting_ops_tests::SESSION,
                    &format!("r{round}"),
                    ASKER,
                ),
                T0,
            )
            .unwrap();
        r.ops.settled(&row.id);
        let item = item_of(&r, &row.id);
        let chat = r.ops.clone();
        let chat_id = row.id.clone();
        let a = std::thread::spawn(move || chat.answer(&chat_id, None, Some(1), By::Mac).unwrap());
        let inbox = r.inbox.clone();
        let item_id = item.id.clone();
        let b = std::thread::spawn(move || {
            inbox
                .decide(&item_id, ActionId::Choice2, "phone")
                .unwrap()
                .answered
        });
        let chat_won = a.join().unwrap() == crate::ask_ops::Taken::Won;
        let inbox_won = b.join().unwrap();
        assert!(chat_won ^ inbox_won, "round {round}");
        r.ops.drain();
        let stored = r.store.get(&row.id).unwrap().unwrap();
        let shown = item_of(&r, &row.id);
        assert_eq!(stored.state, AskState::Answered);
        assert_eq!(shown.state, State::Answered);
        if chat_won {
            chat_wins += 1;
            assert_eq!((stored.by, stored.choice), (Some(By::Mac), Some(1)));
            assert_eq!(shown.answer, Some(ActionId::Choice1));
            assert_eq!(shown.answered_by.as_deref(), Some("mac"));
        } else {
            inbox_wins += 1;
            assert_eq!((stored.by, stored.choice), (Some(By::Phone), Some(2)));
            assert_eq!(shown.answer, Some(ActionId::Choice2));
            assert_eq!(shown.answered_by.as_deref(), Some("phone"));
        }
        r.store
            .close(&row.id, &[AskState::Answered], AskState::Delivered, T0)
            .unwrap();
    }
    assert_eq!(inbox_wins + chat_wins, 300);
}

#[test]
fn cancel_dismisses() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    let done = r.inbox.decide(&item.id, ActionId::Cancel, "phone").unwrap();
    assert!(done.answered);
    r.ops.drain();
    assert_eq!(state_of(&r, &id), "dismissed");
    let item = item_of(&r, &id);
    assert_eq!(item.state, State::Answered);
    assert_eq!(item.answer, Some(ActionId::Cancel));
    assert_eq!(
        wait_as(&r, ASKER, &id, 500).unwrap(),
        json!({"state": "dismissed"})
    );
}

#[test]
fn a_chat_dismiss_resolves_the_item_with_dismiss() {
    let r = rig();
    let id = open(&r);
    person(&r, ops::ASK_DISMISS, json!({"id": id})).unwrap();
    let item = item_of(&r, &id);
    assert_eq!(item.state, State::Answered);
    assert_eq!(item.answer, Some(ActionId::Cancel));
}

#[test]
fn expired_and_gone_withdraw_the_item() {
    let r = rig();
    let gone = open(&r);
    r.tree.end(ASKER);
    r.ops.tick(T0 + 1_000);
    assert_eq!(state_of(&r, &gone), "gone");
    assert_eq!(item_of(&r, &gone).state, State::Withdrawn);

    let r = rig();
    let mut args = crate::ask_testkit::base();
    args["wait"] = json!(true);
    args["deadlineMs"] = json!(T0 + 500);
    let id = crate::ask_testkit::open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    r.ops.tick(T0 + 1_000);
    assert_eq!(state_of(&r, &id), "expired");
    assert_eq!(item_of(&r, &id).state, State::Withdrawn);
}

#[test]
fn load_raises_again_and_drops_the_withdrawn_copy() {
    let data = scratch("inbox-data");
    let inbox_dir = data.0.join("inbox");
    let first = rig_full(
        json!([]),
        Some((
            Arc::new(AskStore::open(&data.0).unwrap()),
            scratch("inbox-a"),
        )),
        Some(inbox_dir.clone()),
    );
    let id = open(&first);
    let old = item_of(&first, &id).id;
    first.inbox.flush();
    drop(first);

    let second = rig_full(
        json!([]),
        Some((
            Arc::new(AskStore::open(&data.0).unwrap()),
            scratch("inbox-b"),
        )),
        Some(inbox_dir),
    );
    second.inbox.load();
    let kept = items(&second);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, old);
    assert_eq!(kept[0].state, State::Withdrawn);
    second.clock.store(T0 + 9_000, Ordering::SeqCst);
    second.ops.load(T0 + 9_000);
    let now = items(&second);
    assert_eq!(now.len(), 1);
    assert_ne!(now[0].id, old);
    assert_eq!(now[0].state, State::Open);
    assert_eq!(now[0].id, item_of(&second, &id).id);
    assert!(now[0].id.starts_with(&format!("ask:{id}:")));
    let choice = second
        .inbox
        .decide(&now[0].id, ActionId::Choice1, "phone")
        .unwrap();
    assert!(choice.answered);
    second.ops.drain();
    assert_eq!(state_of(&second, &id), "answered");
}

#[test]
fn load_removes_the_copy_of_an_answered_row() {
    let data = scratch("inbox-data2");
    let inbox_dir = data.0.join("inbox");
    let first = rig_full(
        json!([]),
        Some((
            Arc::new(AskStore::open(&data.0).unwrap()),
            scratch("inbox-c"),
        )),
        Some(inbox_dir.clone()),
    );
    let id = open(&first);
    first.store.answer(&id, "a", Some(1), By::Mac, T0).unwrap();
    first.inbox.flush();
    drop(first);
    let second = rig_full(
        json!([]),
        Some((
            Arc::new(AskStore::open(&data.0).unwrap()),
            scratch("inbox-d"),
        )),
        Some(inbox_dir),
    );
    second.inbox.load();
    second.ops.load(T0 + 1);
    assert!(items(&second).is_empty());
    assert_eq!(state_of(&second, &id), "answered");
}

#[test]
fn responder_refuses_while_acting_is_off() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    r.acting.set(false);
    let done = r
        .inbox
        .decide(&item.id, ActionId::Choice1, "phone")
        .unwrap();
    assert!(!done.answered);
    let done = r.inbox.decide(&item.id, ActionId::Cancel, "phone").unwrap();
    assert!(!done.answered);
    r.ops.drain();
    assert_eq!(state_of(&r, &id), "open");
    assert_eq!(item_of(&r, &id).state, State::Open);
    r.acting.set(true);
    assert!(
        r.inbox
            .decide(&item.id, ActionId::Choice1, "phone")
            .unwrap()
            .answered
    );
}

#[test]
fn no_answer_text_reaches_the_inbox_or_events() {
    let r = rig();
    let id = open(&r);
    person(&r, ops::ASK_ANSWER, json!({"id": id, "text": TOKEN})).unwrap();
    let dump = serde_json::to_string(&items(&r)).unwrap();
    assert!(!dump.contains(TOKEN));
    for event in of(&r.sink, "session.ask") {
        assert!(text(&event, "answer").is_none());
    }
}

#[test]
fn a_delivered_row_resolves_its_item_as_answered() {
    let r = rig();
    let id = open(&r);
    r.store.answer(&id, "b", Some(2), By::Mac, T0).unwrap();
    r.store
        .close(&id, &[AskState::Answered], AskState::Delivered, T0)
        .unwrap();
    let row = r.store.get(&id).unwrap().unwrap();
    r.ops.resolve_item(&row);
    let item = item_of(&r, &id);
    assert_eq!(item.state, State::Answered);
    assert_eq!(item.answer, Some(ActionId::Choice2));
    assert_eq!(item.answered_by.as_deref(), Some("mac"));
}

#[test]
fn tick_leaves_a_queued_answer_alone() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    r.tree.end(ASKER);
    let done = r.inbox.decide(&item.id, ActionId::Choice1, "mac").unwrap();
    assert!(done.answered);
    r.ops.tick(T0 + 1_000);
    assert_eq!(state_of(&r, &id), "open");
    r.ops.drain();
    assert_eq!(state_of(&r, &id), "answered");
    assert_eq!(item_of(&r, &id).state, State::Answered);
}

#[test]
fn tick_leaves_a_queued_dismiss_alone() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    r.tree.end(ASKER);
    assert!(
        r.inbox
            .decide(&item.id, ActionId::Cancel, "mac")
            .unwrap()
            .answered
    );
    r.ops.tick(T0 + 1_000);
    assert_eq!(state_of(&r, &id), "open");
    r.ops.drain();
    assert_eq!(state_of(&r, &id), "dismissed");
}

#[test]
fn a_queued_answer_that_loses_leaves_no_stale_item() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    assert!(
        r.inbox
            .decide(&item.id, ActionId::Choice1, "mac")
            .unwrap()
            .answered
    );
    r.store
        .close(&id, &[AskState::Open], AskState::Gone, T0 + 1)
        .unwrap();
    r.ops.drain();
    assert_eq!(state_of(&r, &id), "gone");
    assert!(items(&r).is_empty());
    assert!(r.store.get(&id).unwrap().unwrap().inbox_id.is_none());
}

#[test]
fn close_applies_what_is_queued() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    assert!(
        r.inbox
            .decide(&item.id, ActionId::Choice2, "phone")
            .unwrap()
            .answered
    );
    r.ops.close();
    assert_eq!(state_of(&r, &id), "answered");
    assert_eq!(item_of(&r, &id).state, State::Answered);
}

#[test]
fn the_responder_refuses_once_closed() {
    let r = rig();
    let id = open(&r);
    let item = item_of(&r, &id);
    r.ops.close();
    let done = r
        .inbox
        .decide(&item.id, ActionId::Choice1, "phone")
        .unwrap();
    assert!(!done.answered);
    let done = r.inbox.decide(&item.id, ActionId::Cancel, "phone").unwrap();
    assert!(!done.answered);
    r.ops.drain();
    assert_eq!(state_of(&r, &id), "open");
    assert_eq!(item_of(&r, &id).state, State::Open);
}

#[test]
fn load_in_the_same_millisecond_still_gets_a_live_item() {
    let data = scratch("inbox-data3");
    let inbox_dir = data.0.join("inbox");
    let first = rig_full(
        json!([]),
        Some((
            Arc::new(AskStore::open(&data.0).unwrap()),
            scratch("inbox-e"),
        )),
        Some(inbox_dir.clone()),
    );
    let id = open(&first);
    let old = item_of(&first, &id).id;
    first.inbox.flush();
    drop(first);
    let second = rig_full(
        json!([]),
        Some((
            Arc::new(AskStore::open(&data.0).unwrap()),
            scratch("inbox-f"),
        )),
        Some(inbox_dir),
    );
    second.inbox.load();
    second.ops.load(T0);
    let now = items(&second);
    assert_eq!(now.len(), 1);
    assert_ne!(now[0].id, old);
    assert_eq!(now[0].state, State::Open);
    assert!(
        second
            .inbox
            .decide(&now[0].id, ActionId::Choice1, "phone")
            .unwrap()
            .answered
    );
}
