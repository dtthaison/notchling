use super::*;
use crate::chat::outgoing::GRACE_MS;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};
use notchling_local_api::Event;
use serde_json::{json, Value};

fn wire(id: &str, session: &str, state: &str, why: Option<&str>, text: &str) -> Value {
    let mut row = json!({
        "id": id, "session": session, "tool": "claude", "kind": "prompt", "text": text,
        "state": state, "position": 1, "queuedMs": 10_000, "updatedMs": 10_000, "via": "chat",
    });
    if let Some(why) = why {
        row["why"] = json!(why);
    }
    row
}

fn row(id: &str, state: &str, why: Option<&str>, text: &str) -> OutboxRow {
    OutboxRow::from_wire(&wire(id, "ab12cd34", state, why, text)).expect("a row")
}

fn event(name: &str, data: Value) -> Event {
    Event {
        event: name.into(),
        data,
    }
}

fn changed(row: Value) -> Event {
    event("outbox.changed", json!({"row": row}))
}

fn removed(id: &str, session: &str, why: &str) -> Event {
    event(
        "outbox.removed",
        json!({"id": id, "session": session, "why": why}),
    )
}

fn user_items(texts: &[(&str, f64)]) -> Conversation {
    Conversation {
        items: texts
            .iter()
            .enumerate()
            .map(|(i, (t, at))| ChatItem {
                id: format!("i{i}"),
                kind: ItemKind::User {
                    text: (*t).to_owned(),
                    images: 0,
                    merged: false,
                },
                date: Some(*at),
            })
            .collect(),
        ..Conversation::default()
    }
}

#[test]
fn every_state_maps_to_words_and_buttons() {
    use Button::*;
    let cases: Vec<(OutboxRow, Status, Vec<Button>)> = vec![
        (
            row("a", "queued", None, "x"),
            Status::Queued,
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("queue"), "x"),
            Status::WaitsForQueue(Tool::Claude),
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("retry"), "x"),
            Status::QueuedRetry,
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("checking"), "x"),
            Status::Checking,
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("unreadable"), "x"),
            Status::Unreadable,
            vec![ShowTerminal, Edit, Remove],
        ),
        (
            row("a", "waiting", Some("dialog"), "x"),
            Status::WaitingDialog,
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("turn"), "x"),
            Status::WaitsForTurn,
            vec![Edit, Remove],
        ),
        (row("a", "sending", None, "x"), Status::Sending, vec![]),
        (
            row("a", "in_queue", None, "x"),
            Status::QueuedInSession(Tool::Claude),
            vec![],
        ),
        (
            row("a", "unsure", Some("not_found"), "x"),
            Status::MayBeIn,
            vec![Retry, MarkAsSent],
        ),
        (
            row("a", "stopped", None, "x"),
            Status::Stopped,
            vec![ResumeAndSend, Edit, Remove],
        ),
        (row("a", "gone", None, "x"), Status::NotFound, vec![Remove]),
    ];
    for (row, status, buttons) in cases {
        let shown = bubble(&row);
        assert_eq!(shown.status, status, "{:?} {:?}", row.state, row.why);
        assert_eq!(shown.buttons, buttons, "{:?} {:?}", row.state, row.why);
        assert_eq!(shown.text, "x");
        assert_eq!(shown.id, "a");
    }
}

#[test]
fn a_box_row_shows_the_boxes_text_on_one_line_with_its_three_buttons() {
    let mut value = wire("b", "ab12cd34", "box", Some("not_cleared"), "mine");
    value["box"] = json!("my own\ndraft  here");
    let row = OutboxRow::from_wire(&value).unwrap();
    let shown = bubble(&row);
    assert_eq!(shown.status, Status::BoxHasText("my own draft here".into()));
    assert_eq!(
        shown.buttons,
        vec![Button::ClearBoxAndSend, Button::Edit, Button::Remove]
    );
    let bare = OutboxRow::from_wire(&wire("c", "s", "box", Some("mixed"), "t")).unwrap();
    assert_eq!(bubble(&bare).status, Status::BoxHasText(String::new()));
}

#[test]
fn a_row_with_a_word_this_app_does_not_know_is_skipped() {
    assert!(OutboxRow::from_wire(&wire("a", "s", "teleporting", None, "x")).is_none());
    assert!(OutboxRow::from_wire(&json!({"id": "a"})).is_none());
}

#[test]
fn buttons_that_act_without_text_make_their_request() {
    assert_eq!(
        Button::Remove.request("o1", "ab12").unwrap().args,
        json!({"id": "o1", "session": "ab12"})
    );
    assert_eq!(
        Button::MarkAsSent.request("o1", "ab12").unwrap().args,
        json!({"id": "o1", "session": "ab12", "sent": true})
    );
    let again = Button::Retry.request("o1", "ab12").unwrap();
    assert_eq!(
        (again.op, again.args),
        ("outbox.resend", json!({"id": "o1", "session": "ab12"}))
    );
    assert_eq!(
        Button::ClearBoxAndSend
            .request("o1", "n1/ab12")
            .unwrap()
            .args,
        json!({"id": "o1", "session": "n1/ab12", "clear_box": true})
    );
    assert_eq!(
        Button::ResumeAndSend.request("o1", "ab12").unwrap().args,
        json!({"id": "o1", "session": "ab12", "resume": true})
    );
    assert!(Button::Edit.request("o1", "ab12").is_none());
}

#[test]
fn in_queue_row_hides_matching_conversation_queued_entry() {
    let queued = vec![
        "first".to_owned(),
        "ok  then".to_owned(),
        "ok then".to_owned(),
    ];
    let held = [row("a", "in_queue", None, "ok then")];
    assert_eq!(hidden_queued(&queued, &held), Some(1));
    let waiting = [row("a", "queued", None, "ok then")];
    assert_eq!(hidden_queued(&queued, &waiting), None);
    let other = [row("a", "in_queue", None, "something else")];
    assert_eq!(hidden_queued(&queued, &other), None);
    assert_eq!(hidden_queued(&[], &held), None);
}

#[test]
fn changed_and_removed_apply_in_order() {
    let mut feed = OutboxFeed::default();
    assert!(feed.apply(&changed(wire("a", "s1", "queued", None, "one")), 0));
    assert!(feed.apply(&changed(wire("b", "s1", "queued", None, "two")), 0));
    assert!(feed.apply(&changed(wire("c", "s2", "queued", None, "other")), 0));
    let ids = |feed: &OutboxFeed, s: &str| -> Vec<String> {
        feed.rows(s).iter().map(|r| r.id.clone()).collect()
    };
    assert_eq!(ids(&feed, "s1"), ["a", "b"]);
    assert_eq!(ids(&feed, "s2"), ["c"]);
    assert!(feed.apply(&changed(wire("a", "s1", "sending", None, "one")), 0));
    assert_eq!(feed.rows("s1")[0].state, State::Sending);
    assert_eq!(feed.rows("s1").len(), 2);
    assert!(feed.apply(&removed("a", "s1", "removed"), 0));
    assert_eq!(ids(&feed, "s1"), ["b"]);
    assert!(feed.apply(&removed("zzz", "s1", "removed"), 0));
    assert!(!feed.apply(&removed("zzz", "s1", "removed"), 0));
    assert_eq!(ids(&feed, "s1"), ["b"]);
    assert!(feed.rows("nobody").is_empty());
    assert!(!feed.apply(&event("outbox.other", json!({})), 0));
}

#[test]
fn reconnect_reseeds_from_list() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("old", "s1", "queued", None, "gone")), 0);
    feed.seed(&[
        wire("n1", "s1", "queued", None, "one"),
        wire("n2", "s2", "waiting", Some("dialog"), "two"),
    ]);
    assert_eq!(feed.rows("s1").len(), 1);
    assert_eq!(feed.rows("s1")[0].id, "n1");
    assert_eq!(feed.rows("s2")[0].why, Some(Why::Dialog));
}

#[test]
fn sent_bubble_stays_until_transcript_shows_it() {
    let mut feed = OutboxFeed::default();
    feed.apply(
        &changed(wire("a", "s1", "sending", None, "ship it")),
        10_000,
    );
    assert!(feed.apply(&removed("a", "s1", "sent"), 12_000));
    assert!(feed.rows("s1").is_empty());
    let shown = feed.bubbles("s1");
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].status, Status::Sent);
    assert!(shown[0].buttons.is_empty());

    let before = user_items(&[("something older", 1.0)]);
    feed.settle("s1", &before, 13_000);
    assert_eq!(
        feed.bubbles("s1").len(),
        1,
        "the transcript has no copy yet"
    );

    let after = user_items(&[("something older", 1.0), ("ship it\n", 12.5)]);
    feed.settle("s1", &after, 14_000);
    assert!(feed.bubbles("s1").is_empty());
}

#[test]
fn an_older_identical_message_does_not_end_the_sent_bubble() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("a", "s1", "sending", None, "ok")), 100_000);
    feed.apply(&removed("a", "s1", "sent"), 102_000);
    let older = user_items(&[("ok", 5.0)]);
    feed.settle("s1", &older, 103_000);
    assert_eq!(feed.bubbles("s1").len(), 1);
}

#[test]
fn a_sent_bubble_the_transcript_never_shows_lets_go_after_the_grace() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("a", "s1", "sending", None, "words")), 0);
    feed.apply(&removed("a", "s1", "sent"), 1_000);
    let c = user_items(&[]);
    feed.settle("s1", &c, 1_000 + GRACE_MS - 1);
    assert_eq!(feed.bubbles("s1").len(), 1);
    feed.settle("s1", &c, 1_000 + GRACE_MS);
    assert!(feed.bubbles("s1").is_empty());
}

#[test]
fn sent_bubbles_come_before_the_rows_still_waiting() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("a", "s1", "sending", None, "first")), 0);
    feed.apply(&changed(wire("b", "s1", "queued", None, "second")), 0);
    feed.sent("a", 500);
    let ids: Vec<String> = feed.bubbles("s1").into_iter().map(|b| b.id).collect();
    assert_eq!(ids, ["a", "b"]);
}

#[test]
fn a_removed_row_that_was_never_seen_leaves_no_sent_bubble() {
    let mut feed = OutboxFeed::default();
    assert!(!feed.apply(&removed("ghost", "s1", "sent"), 0));
    assert!(feed.bubbles("s1").is_empty());
}

#[test]
fn a_row_that_comes_back_after_sent_is_not_kept_as_sent() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("a", "s1", "sending", None, "again")), 0);
    feed.apply(&removed("a", "s1", "sent"), 1_000);
    assert_eq!(feed.bubbles("s1")[0].status, Status::Sent);
    assert!(feed.apply(&changed(wire("a", "s1", "queued", None, "again")), 2_000));
    let shown = feed.bubbles("s1");
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].status, Status::Queued);
    assert_eq!(shown[0].buttons, vec![Button::Edit, Button::Remove]);
    feed.apply(&removed("a", "s1", "removed"), 3_000);
    assert!(feed.bubbles("s1").is_empty());
    assert!(feed.is_empty());
}

#[test]
fn reseeding_rows_drops_sent_entries_whose_row_is_back() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("a", "s1", "sending", None, "one")), 0);
    feed.apply(&changed(wire("b", "s1", "sending", None, "two")), 0);
    feed.apply(&removed("a", "s1", "sent"), 1_000);
    feed.apply(&removed("b", "s1", "sent"), 1_000);
    let mut fresh = OutboxFeed::default();
    fresh.seed(&[wire("a", "s1", "unsure", Some("not_found"), "one")]);
    feed.reseed_rows(fresh);
    let shown = feed.bubbles("s1");
    let kinds: Vec<(&str, Status)> = shown
        .iter()
        .map(|b| (b.id.as_str(), b.status.clone()))
        .collect();
    assert_eq!(kinds, [("b", Status::Sent), ("a", Status::MayBeIn)]);
}

fn pending(text: &str) -> Outgoing {
    Outgoing::new(text, &Conversation::default(), 0)
}

fn outgoing_texts(list: &[Outgoing]) -> Vec<String> {
    list.iter().map(|o| o.text.clone()).collect()
}

#[test]
fn an_outgoing_goes_once_the_feed_shows_its_row_so_an_edit_leaves_no_ghost() {
    let mut feed = OutboxFeed::default();
    let mut list = vec![pending("ok then")];
    deliver(&mut list, "ok then", 100, Some("o1"));
    assert_eq!(list[0].boxed.as_deref(), Some("o1"));
    drop_boxed(&mut list, |id| feed.shows(id));
    assert_eq!(list.len(), 1, "the row has not arrived yet");

    feed.apply(&changed(wire("o1", "s1", "queued", None, "ok then")), 200);
    drop_boxed(&mut list, |id| feed.shows(id));
    assert!(list.is_empty());

    feed.apply(&changed(wire("o1", "s1", "queued", None, "ok, then")), 300);
    let bubbles = feed.bubbles("s1");
    assert_eq!(bubbles[0].text, "ok, then");
    assert!(unbubbled(outgoing_texts(&list), &bubbles).is_empty());
}

#[test]
fn a_removed_row_leaves_no_outgoing_bubble_behind() {
    let mut feed = OutboxFeed::default();
    let mut list = vec![pending("remove me")];
    deliver(&mut list, "remove me", 100, Some("o1"));
    feed.apply(&changed(wire("o1", "s1", "queued", None, "remove me")), 200);
    drop_boxed(&mut list, |id| feed.shows(id));
    feed.apply(&removed("o1", "s1", "removed"), 300);
    assert!(feed.bubbles("s1").is_empty());
    assert!(unbubbled(outgoing_texts(&list), &feed.bubbles("s1")).is_empty());
}

fn lists(entries: &[(&str, &str, &str)]) -> std::collections::HashMap<String, Vec<Outgoing>> {
    let mut lists: std::collections::HashMap<String, Vec<Outgoing>> = Default::default();
    for (chat, text, row) in entries {
        let list = lists.entry((*chat).to_owned()).or_default();
        list.push(pending(text));
        deliver(list, text, 100, Some(row));
    }
    lists
}

#[test]
fn a_row_removed_before_the_feed_showed_it_drops_its_outgoing_in_any_session() {
    let mut feed = OutboxFeed::default();
    let mut all = lists(&[
        ("job-a", "vanishes", "o1"),
        ("job-b", "stays", "o2"),
        ("job-c", "edited away", "o3"),
    ]);
    assert!(feed.apply(&removed("o1", "s1", "removed"), 200));
    feed.apply(
        &changed(wire("o3", "s3", "queued", None, "edited away")),
        210,
    );
    drop_covered(&mut all, |id| feed.covers(id));
    assert!(all["job-a"].is_empty());
    assert_eq!(outgoing_texts(&all["job-b"]), ["stays"]);
    assert!(all["job-c"].is_empty());
}

#[test]
fn a_sent_message_for_a_row_never_shown_keeps_its_outgoing_until_the_transcript_has_it() {
    let mut feed = OutboxFeed::default();
    let mut all = lists(&[("job-a", "went out", "o1")]);
    assert!(!feed.apply(&removed("o1", "s1", "sent"), 200));
    drop_covered(&mut all, |id| feed.covers(id));
    assert_eq!(outgoing_texts(&all["job-a"]), ["went out"]);
}

#[test]
fn the_memory_of_removed_rows_is_bounded_and_forgets_the_oldest() {
    let mut feed = OutboxFeed::default();
    for n in 0..GONE_KEPT + 5 {
        feed.apply(&removed(&format!("r{n}"), "s1", "removed"), 0);
    }
    assert!(!feed.covers("r0"));
    assert!(!feed.covers("r4"));
    assert!(feed.covers("r5"));
    assert!(feed.covers(&format!("r{}", GONE_KEPT + 4)));
}

#[test]
fn rows_arriving_out_of_order_are_placed_in_queue_order() {
    let mut feed = OutboxFeed::default();
    let at = |id: &str, queued: i64, position: u64| {
        let mut value = wire(id, "s1", "queued", None, id);
        value["queuedMs"] = json!(queued);
        value["position"] = json!(position);
        value
    };
    feed.apply(&changed(at("c", 3_000, 3)), 0);
    feed.apply(&changed(at("a", 1_000, 1)), 0);
    feed.apply(&changed(at("b", 2_000, 2)), 0);
    let ids: Vec<&str> = feed.rows("s1").iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
    feed.apply(&changed(at("b", 2_000, 2)), 0);
    feed.apply(&changed(wire("b", "s1", "sending", None, "b")), 0);
    let ids: Vec<&str> = feed.rows("s1").iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"], "an update keeps its place");
}

#[test]
fn rows_with_the_same_time_keep_their_arrival_order_unless_the_position_says_otherwise() {
    let mut feed = OutboxFeed::default();
    let at = |id: &str, position: u64| {
        let mut value = wire(id, "s1", "queued", None, id);
        value["position"] = json!(position);
        value
    };
    feed.apply(&changed(at("x", 2)), 0);
    feed.apply(&changed(at("y", 1)), 0);
    feed.apply(&changed(at("z", 3)), 0);
    let ids: Vec<&str> = feed.rows("s1").iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["y", "x", "z"]);
}

#[test]
fn a_head_that_needs_you_is_the_first_in_queue_order_not_in_arrival_order() {
    let mut feed = OutboxFeed::default();
    let at = |id: &str, state: &str, queued: i64| {
        let mut value = wire(id, "ab12cd34", state, None, id);
        value["queuedMs"] = json!(queued);
        value
    };
    feed.apply(&changed(at("late", "queued", 9_000)), 0);
    feed.apply(&changed(at("early", "unsure", 1_000)), 0);
    let agent = job_agent("ab12cd34", "ab12cd34");
    assert!(feed.needs_you(&agent));
}

#[test]
fn a_sent_row_counts_as_shown_while_its_sent_bubble_stays() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("o1", "s1", "sending", None, "ship")), 0);
    feed.apply(&removed("o1", "s1", "sent"), 10);
    assert!(feed.shows("o1"));
    assert!(!feed.shows("o2"));
    let mut list = vec![pending("ship")];
    deliver(&mut list, "ship", 20, Some("o1"));
    drop_boxed(&mut list, |id| feed.shows(id));
    assert!(list.is_empty());
}

#[test]
fn only_the_delivered_copy_is_tied_to_its_row() {
    let mut list = vec![pending("ok"), pending("ok"), pending("other")];
    deliver(&mut list, "ok", 100, Some("o1"));
    assert_eq!(list[0].boxed.as_deref(), Some("o1"));
    assert_eq!(list[0].delivered_at, Some(100));
    assert_eq!(list[1].boxed, None);
    assert_eq!(list[1].delivered_at, None);
    deliver(&mut list, "ok", 150, None);
    assert_eq!(list[1].delivered_at, Some(150));
    assert_eq!(list[1].boxed, None);
    drop_boxed(&mut list, |_| true);
    assert_eq!(outgoing_texts(&list), ["ok", "other"]);
}

#[test]
fn forgetting_a_row_drops_only_the_outgoing_tied_to_it() {
    let mut list = vec![pending("a"), pending("b"), pending("c")];
    deliver(&mut list, "a", 1, Some("o1"));
    deliver(&mut list, "b", 1, Some("o2"));
    forget(&mut list, "o1");
    assert_eq!(outgoing_texts(&list), ["b", "c"]);
    forget(&mut list, "zzz");
    assert_eq!(list.len(), 2);
}

fn job_agent(id: &str, job: &str) -> Agent {
    Agent {
        id: id.into(),
        name: "job".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job(job.into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

#[test]
fn agent_accessors_follow_the_outbox_key_not_the_row_id() {
    let agent = job_agent("6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c", "ab12cd34");
    let mut feed = OutboxFeed::default();
    feed.seed(&[wire("o1", "ab12cd34", "queued", None, "hello")]);
    assert_eq!(feed.rows_for(&agent).len(), 1);
    assert_eq!(feed.bubbles_for(&agent)[0].text, "hello");
    assert!(feed.rows("6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c").is_empty());

    feed.apply(&removed("o1", "ab12cd34", "sent"), 20_000);
    assert_eq!(feed.bubbles_for(&agent)[0].status, Status::Sent);
    let shown = user_items(&[("hello", 21.0)]);
    assert!(feed.settle_for(&agent, &shown, 22_000));
    assert!(feed.bubbles_for(&agent).is_empty());
}

#[test]
fn an_outgoing_text_is_hidden_once_the_feed_shows_its_bubble_one_for_one() {
    let bubbles = vec![bubble(&row("o1", "queued", None, "ok  then"))];
    let outgoing = vec![
        "ok then".to_owned(),
        "ok then".to_owned(),
        "other".to_owned(),
    ];
    assert_eq!(unbubbled(outgoing, &bubbles), ["ok then", "other"]);
    assert_eq!(unbubbled(vec!["x".to_owned()], &[]), ["x"]);
}

#[test]
fn a_combined_queued_row_is_one_bubble_in_its_place_and_both_sends_leave_no_ghost() {
    let mut feed = OutboxFeed::default();
    let mut list = vec![pending("first")];
    deliver(&mut list, "first", 100, Some("o1"));
    let mut first = wire("o1", "s1", "queued", None, "first");
    first["queuedMs"] = json!(1_000);
    feed.apply(&changed(first), 200);
    let mut later = wire("o2", "s1", "queued", None, "later");
    later["queuedMs"] = json!(2_000);
    later["position"] = json!(2);
    feed.apply(&changed(later), 200);

    list.push(pending("second"));
    assert_eq!(
        unbubbled(outgoing_texts(&list), &feed.bubbles("s1")),
        ["second"],
        "until the answer, the second send shows once"
    );
    deliver(&mut list, "second", 300, Some("o1"));
    let mut joined = wire("o1", "s1", "queued", None, "first\n\nsecond");
    joined["queuedMs"] = json!(1_000);
    joined["updatedMs"] = json!(20_000);
    assert!(feed.apply(&changed(joined), 400));
    drop_boxed(&mut list, |id| feed.shows(id));
    assert!(list.is_empty());

    let rows = feed.rows("s1");
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["o1", "o2"], "the joined row keeps its place");
    let bubbles = feed.bubbles("s1");
    assert_eq!(bubbles.len(), 2);
    assert_eq!(bubbles[0].text, "first\n\nsecond");
    assert_eq!(bubbles[0].status, Status::Queued);
    assert_eq!(bubbles[0].buttons, [Button::Edit, Button::Remove]);
}

#[test]
fn rows_the_daemon_joined_leave_one_bubble_and_no_ghost_of_any_send() {
    let mut feed = OutboxFeed::default();
    let mut list = Vec::new();
    for (n, (id, text)) in [("o1", "one"), ("o2", "two"), ("o3", "three")]
        .into_iter()
        .enumerate()
    {
        list.push(pending(text));
        deliver(&mut list, text, 100, Some(id));
        let mut row = wire(id, "s1", "queued", None, text);
        row["queuedMs"] = json!(1_000 * (n as i64 + 1));
        row["position"] = json!(n + 1);
        feed.apply(&changed(row), 200);
    }
    assert_eq!(feed.bubbles("s1").len(), 3);
    let mut head = wire("o1", "s1", "sending", None, "one\n\ntwo\n\nthree");
    head["queuedMs"] = json!(1_000);
    head["joined"] = json!(3);
    assert!(feed.apply(&changed(head), 300));
    for id in ["o2", "o3"] {
        assert!(feed.apply(&removed(id, "s1", "joined"), 310));
    }
    drop_boxed(&mut list, |id| feed.covers(id));
    assert!(
        list.is_empty(),
        "no send is drawn again beside the joined one"
    );
    let bubbles = feed.bubbles("s1");
    assert_eq!(bubbles.len(), 1);
    assert_eq!(bubbles[0].text, "one\n\ntwo\n\nthree");
    assert_eq!(bubbles[0].joined, 3);
    assert_eq!(
        bubbles[0].text.split("\n\n").count(),
        bubbles[0].joined as usize
    );
    assert_eq!(bubbles[0].status, Status::Sending);
    assert!(unbubbled(outgoing_texts(&list), &bubbles).is_empty());
}
