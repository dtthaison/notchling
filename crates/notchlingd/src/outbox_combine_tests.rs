use super::*;
use notchling_engine::outbox::MAX_TEXT;

fn then_of(r: &Rig) -> Vec<(String, String)> {
    of(&r.sink, "session.outbox")
        .iter()
        .map(|e| (text(e, "then").unwrap(), text(e, "chars").unwrap()))
        .collect()
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

#[test]
fn a_second_send_joins_the_queued_row_instead_of_a_new_one() {
    let r = rig();
    let first = send(&r, "job-idle", "first");
    let second = send(&r, "job-idle", "second");
    assert_eq!(id_of(&second), id_of(&first));
    assert_eq!(second["state"], "queued");
    assert_eq!(second["position"], 1);
    assert_eq!(second["combined"], true);
    assert!(first.get("combined").is_none());
    let rows = r.store.list(Some("idle0001")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "first\n\nsecond");
    let changed = r.room.named("outbox.changed");
    assert_eq!(changed.len(), 2);
    assert_eq!(changed[1]["row"]["id"], json!(rows[0].id));
    assert_eq!(changed[1]["row"]["text"], "first\n\nsecond");
    assert_eq!(changed[1]["row"]["state"], "queued");
    assert_eq!(
        then_of(&r),
        [pair("queued", "5"), pair("combined", "6")],
        "the second event counts the added message"
    );
}

#[test]
fn the_join_wakes_the_postman_and_tells_peers() {
    let r = rig();
    let woken = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = woken.clone();
    r.ops
        .set_wake(Arc::new(move |s| seen.lock().unwrap().push(s.to_string())));
    let told = Arc::new(Mutex::new(Vec::<String>::new()));
    let heard = told.clone();
    r.ops.set_tell(Arc::new(move |_, name, data| {
        assert!(!data.to_string().contains("second"));
        heard.lock().unwrap().push(name.to_string());
    }));
    send(&r, "job-idle", "first");
    send(&r, "job-idle", "second");
    assert_eq!(woken.lock().unwrap().len(), 2);
    assert_eq!(told.lock().unwrap().len(), 2);
}

#[test]
fn a_send_after_the_postman_took_the_row_is_a_new_row() {
    let r = rig();
    let first = id_of(&send(&r, "job-idle", "first"));
    r.store.take(&first, 10).unwrap().unwrap();
    let second = send(&r, "job-idle", "second");
    assert_ne!(id_of(&second), first);
    assert_eq!(second["position"], 2);
    assert!(second.get("combined").is_none());
    let rows = r.store.list(Some("idle0001")).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].text, "first");
    assert_eq!(rows[1].text, "second");
}

#[test]
fn a_third_send_joins_the_new_last_row_not_the_head() {
    let r = rig();
    let first = id_of(&send(&r, "job-idle", "first"));
    r.store.take(&first, 10).unwrap().unwrap();
    let second = id_of(&send(&r, "job-idle", "second"));
    let third = send(&r, "job-idle", "third");
    assert_eq!(id_of(&third), second);
    assert_eq!(third["position"], 2);
    let rows = r.store.list(Some("idle0001")).unwrap();
    assert_eq!(rows[0].text, "first");
    assert_eq!(rows[1].text, "second\n\nthird");
}

#[test]
fn another_session_gets_its_own_row() {
    let r = rig();
    send(&r, "job-idle", "one");
    let other = send(&r, "codex:t1", "two");
    assert_eq!(other["position"], 1);
    assert!(other.get("combined").is_none());
    assert_eq!(r.store.list(None).unwrap().len(), 2);
}

#[test]
fn codex_and_antigravity_messages_join_too() {
    let r = rig();
    for id in ["codex:t1", "agy:c9"] {
        let first = send(&r, id, "one");
        let second = send(&r, id, "two");
        assert_eq!(id_of(&second), id_of(&first), "{id}");
    }
    let rows = r.store.list(None).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].text, "one\n\ntwo");
    assert_eq!(rows[1].text, "one\n\ntwo");
}

#[test]
fn a_command_and_a_menu_refusal_do_not_join() {
    let r = rig();
    send(&r, "job-idle", "hello");
    let command = send(&r, "job-idle", "/compact");
    assert_eq!(command["position"], 2);
    let after = send(&r, "job-idle", "more");
    assert_eq!(after["position"], 3);
    assert_eq!(r.store.list(None).unwrap().len(), 3);
    let menu = call(
        &r,
        ops::SESSION_SEND,
        json!({"id": "job-idle", "text": "/model"}),
    );
    assert_eq!(code(menu).0, ErrorCode::BadRequest);
    assert_eq!(r.store.list(None).unwrap().len(), 3);
}

#[test]
fn a_clear_box_send_and_a_different_option_set_do_not_join() {
    let r = rig();
    send(&r, "codex:t1", "one");
    let with_model = call(
        &r,
        ops::SESSION_SEND,
        json!({"id": "codex:t1", "text": "two", "options": {"model": "m"}}),
    )
    .unwrap();
    assert_eq!(with_model["position"], 2);
    send(&r, "job-idle", "one");
    let clearing = call(
        &r,
        ops::SESSION_SEND,
        json!({"id": "job-idle", "text": "two", "clear_box": true}),
    )
    .unwrap();
    assert_eq!(clearing["position"], 2);
}

#[test]
fn a_message_over_the_size_limit_after_joining_is_a_new_row() {
    let r = rig();
    let big = "x".repeat(MAX_TEXT - 4);
    send(&r, "job-idle", &big);
    let second = send(&r, "job-idle", "too much");
    assert_eq!(second["position"], 2);
    assert!(second.get("combined").is_none());
    let rows = r.store.list(None).unwrap();
    assert_eq!(rows[0].text.len(), MAX_TEXT - 4);
    assert_eq!(rows[1].text, "too much");
}

#[test]
fn an_oversized_or_empty_send_is_refused_before_any_join() {
    let r = rig();
    send(&r, "job-idle", "one");
    let empty = call(
        &r,
        ops::SESSION_SEND,
        json!({"id": "job-idle", "text": "  "}),
    );
    assert_eq!(code(empty).0, ErrorCode::BadRequest);
    let long = "y".repeat(MAX_TEXT + 1);
    let long = call(
        &r,
        ops::SESSION_SEND,
        json!({"id": "job-idle", "text": long}),
    );
    assert_eq!(code(long).0, ErrorCode::BadRequest);
    assert_eq!(r.store.list(None).unwrap()[0].text, "one");
}

#[test]
fn a_joined_row_edits_and_removes_as_one() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "a"));
    send(&r, "job-idle", "b");
    call(&r, ops::OUTBOX_EDIT, json!({"id": id, "text": "a and b"})).unwrap();
    assert_eq!(r.store.list(None).unwrap()[0].text, "a and b");
    send(&r, "job-idle", "c");
    assert_eq!(r.store.list(None).unwrap()[0].text, "a and b\n\nc");
    call(&r, ops::OUTBOX_REMOVE, json!({"id": id})).unwrap();
    assert!(r.store.list(None).unwrap().is_empty());
}

#[test]
fn pictures_added_as_paths_keep_their_order() {
    let r = rig();
    send(&r, "job-idle", "look\n\n/tmp/one.png");
    send(&r, "job-idle", "and\n\n/tmp/two.png");
    assert_eq!(
        r.store.list(None).unwrap()[0].text,
        "look\n\n/tmp/one.png\n\nand\n\n/tmp/two.png"
    );
}

#[test]
fn a_send_does_not_join_a_row_a_reply_is_waiting_on() {
    let r = rig();
    let reply = r
        .ops
        .send(&crate::outbox_ops::SendArgs {
            id: "job-idle".into(),
            text: "from a reply".into(),
            clear_box: false,
            options: None,
            via: notchling_engine::outbox::Via::Agent,
        })
        .unwrap();
    let sent = send(&r, "job-idle", "typed in chat");
    assert_ne!(id_of(&sent), reply.id);
    assert_eq!(sent["position"], 2);
    assert!(sent.get("combined").is_none());
    let rows = r.store.list(None).unwrap();
    assert_eq!(rows[0].text, "from a reply");
    assert_eq!(rows[1].text, "typed in chat");
    assert!(r.ops.give_back(&rows[0]));
    let third = send(&r, "job-idle", "and more");
    assert_eq!(id_of(&third), id_of(&sent));
}

#[test]
fn the_combined_event_names_who_added_the_text() {
    let r = rig();
    send(&r, "job-idle", "first");
    call(
        &r,
        ops::SESSION_SEND,
        json!({"id": "job-idle", "text": "second", "via": "phone"}),
    )
    .unwrap();
    let events = of(&r.sink, "session.outbox");
    assert_eq!(text(&events[0], "via").as_deref(), Some("chat"));
    assert_eq!(text(&events[1], "then").as_deref(), Some("combined"));
    assert_eq!(text(&events[1], "via").as_deref(), Some("phone"));
}

#[test]
fn a_reply_does_not_join_a_queued_row() {
    let r = rig();
    send(&r, "job-idle", "typed in chat");
    let sent = r
        .ops
        .send(&crate::outbox_ops::SendArgs {
            id: "job-idle".into(),
            text: "from a reply".into(),
            clear_box: false,
            options: None,
            via: notchling_engine::outbox::Via::Agent,
        })
        .unwrap();
    assert_eq!(r.store.list(None).unwrap().len(), 2);
    assert_eq!(sent.text, "from a reply");
}

#[test]
fn a_send_joins_a_row_waiting_for_the_turn_and_wakes_the_postman() {
    let r = rig();
    let first = id_of(&send(&r, "job-idle", "first"));
    r.store
        .set_state(
            &first,
            &[State::Queued],
            State::Waiting,
            Some(Why::Turn),
            Some(99),
            5,
        )
        .unwrap();
    let woken = Arc::new(Mutex::new(0));
    let seen = woken.clone();
    r.ops
        .set_wake(Arc::new(move |_| *seen.lock().unwrap() += 1));
    let second = send(&r, "job-idle", "second");
    assert_eq!(id_of(&second), first);
    assert_eq!(second["combined"], true);
    let row = r.store.get(&first).unwrap().unwrap();
    assert_eq!((row.state, row.why), (State::Waiting, Some(Why::Turn)));
    assert_eq!(row.text, "first\n\nsecond");
    assert_eq!(*woken.lock().unwrap(), 1);
    let changed = r.room.named("outbox.changed");
    assert_eq!(changed.last().unwrap()["row"]["why"], "turn");
    assert_eq!(changed.last().unwrap()["row"]["text"], "first\n\nsecond");
}

#[test]
fn a_send_does_not_join_a_row_waiting_to_retry() {
    let r = rig();
    let first = id_of(&send(&r, "job-idle", "first"));
    r.store
        .set_state(
            &first,
            &[State::Queued],
            State::Waiting,
            Some(Why::Retry),
            Some(99),
            5,
        )
        .unwrap();
    let second = send(&r, "job-idle", "second");
    assert_ne!(id_of(&second), first);
    assert_eq!(second["position"], 2);
}

#[test]
fn an_edit_from_the_text_it_started_with_saves_and_a_stale_one_is_refused() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "first"));
    send(&r, "job-idle", "second");
    let stale = call(
        &r,
        ops::OUTBOX_EDIT,
        json!({"id": id, "text": "first, edited", "from": "first"}),
    );
    assert_eq!(
        code(stale),
        (ErrorCode::Failed, "It changed while you edited it.".into())
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().text, "first\n\nsecond");
    let before = r.room.named("outbox.changed").len();
    let fresh = call(
        &r,
        ops::OUTBOX_EDIT,
        json!({"id": id, "text": "both, edited", "from": "first\n\nsecond"}),
    );
    assert_eq!(fresh.unwrap(), json!({"edited": true}));
    assert_eq!(r.store.get(&id).unwrap().unwrap().text, "both, edited");
    assert_eq!(r.room.named("outbox.changed").len(), before + 1);
    let old_client = call(&r, ops::OUTBOX_EDIT, json!({"id": id, "text": "no from"}));
    assert!(old_client.is_ok());
    assert_eq!(r.store.get(&id).unwrap().unwrap().text, "no from");
}

#[test]
fn a_stale_edit_publishes_nothing_and_logs_no_edit() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "first"));
    send(&r, "job-idle", "second");
    let changed = r.room.named("outbox.changed").len();
    let events = of(&r.sink, "session.outbox").len();
    let _ = call(
        &r,
        ops::OUTBOX_EDIT,
        json!({"id": id, "text": "x", "from": "first"}),
    );
    assert_eq!(r.room.named("outbox.changed").len(), changed);
    assert_eq!(of(&r.sink, "session.outbox").len(), events);
}

#[test]
fn a_sending_row_is_refused_as_being_sent_even_with_a_stale_from() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "first"));
    r.store.take(&id, 10).unwrap().unwrap();
    let refused = call(
        &r,
        ops::OUTBOX_EDIT,
        json!({"id": id, "text": "x", "from": "something else"}),
    );
    assert_eq!(code(refused).1, notchling_engine::outbox::BEING_SENT);
}
