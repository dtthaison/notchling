use super::*;
use serde_json::json;

fn row(id: &str, choices: &[&str]) -> AskRow {
    AskRow::from_wire(&json!({
        "id": id, "session": "ab12cd34", "asker": "subagent", "label": "Review Task 3",
        "question": "Which one?", "choices": choices,
        "mainMayAnswer": false, "state": "open", "waiting": true,
        "createdMs": 1_000_000, "updatedMs": 1_000_000,
    }))
    .expect("a row")
}

fn closed(id: &str) -> AskRow {
    let mut r = row(id, &["Keep it", "Drop it"]);
    r.state = notchling_engine::ask::AskState::Dismissed;
    r
}

fn type_all(drafts: &mut Drafts, r: &AskRow, text: &str) {
    for c in text.chars() {
        let s = c.to_string();
        assert_eq!(drafts.key(r, &s, Some(&s), false), Keyed::Changed);
    }
}

#[test]
fn ask_card_actions_send_the_right_requests() {
    let r = row("q1", &["Keep it", "Drop it"]);
    let mut drafts = Drafts::default();
    assert_eq!(
        drafts.act(&r, Act::Choose(2)),
        Some(Request::ask_answer("q1", AskAnswer::Choice(2)))
    );
    drafts.settle("q1", Ok(json!({"answered": true})));
    type_all(&mut drafts, &r, "use the second");
    assert_eq!(drafts.typed("q1"), "use the second");
    assert_eq!(
        drafts.act(&r, Act::Send),
        Some(Request::ask_answer("q1", AskAnswer::Text("use the second")))
    );
    assert_eq!(drafts.typed("q1"), "", "Send clears the box");
    drafts.settle("q1", Ok(json!({"answered": true})));
    assert_eq!(
        drafts.act(&r, Act::Dismiss),
        Some(Request::ask_dismiss("q1"))
    );
    drafts.settle("q1", Ok(json!({"dismissed": true})));
    assert_eq!(drafts.lost("q1"), None);
    assert_eq!(drafts.act(&r, Act::Choose(3)), None, "no such choice");
    assert_eq!(drafts.act(&r, Act::Choose(0)), None);
    assert_eq!(
        drafts.act(&r, Act::Send),
        None,
        "an empty box sends nothing"
    );
    type_all(&mut drafts, &r, "   ");
    assert_eq!(drafts.act(&r, Act::Send), None, "blanks send nothing");
}

#[test]
fn a_lost_answer_says_who_answered_first() {
    let r = row("q1", &["Keep it", "Drop it"]);
    let mut drafts = Drafts::default();
    assert!(drafts.act(&r, Act::Choose(1)).is_some());
    assert!(drafts.busy("q1"));
    drafts.settle(
        "q1",
        Ok(json!({"answered": false, "state": "answered", "by": "phone"})),
    );
    assert!(!drafts.busy("q1"));
    assert_eq!(drafts.lost("q1"), Some("Already answered · Phone"));
    assert_eq!(drafts.lost("q2"), None);
    drafts.settle("q1", Ok(json!({"answered": true})));
    assert_eq!(drafts.lost("q1"), None, "a later success clears the note");
}

#[test]
fn a_failed_send_keeps_a_note_and_frees_the_card() {
    let r = row("q1", &["Keep it", "Drop it"]);
    let mut drafts = Drafts::default();
    assert!(drafts.act(&r, Act::Choose(1)).is_some());
    drafts.settle("q1", Err("Couldn't save the answer.".into()));
    assert!(!drafts.busy("q1"));
    assert!(drafts
        .lost("q1")
        .is_some_and(|line| line.contains("Couldn't save the answer")));
    assert!(
        drafts.act(&r, Act::Choose(1)).is_some(),
        "it can be tried again"
    );
}

#[test]
fn a_card_in_flight_or_closed_sends_nothing_more() {
    let r = row("q1", &["Keep it", "Drop it"]);
    let mut drafts = Drafts::default();
    assert!(drafts.act(&r, Act::Choose(1)).is_some());
    assert_eq!(drafts.act(&r, Act::Choose(2)), None, "one at a time");
    assert_eq!(drafts.act(&r, Act::Dismiss), None);
    let gone = closed("q2");
    assert_eq!(drafts.act(&gone, Act::Choose(1)), None);
    assert_eq!(drafts.act(&gone, Act::Dismiss), None);
    assert_eq!(drafts.key(&gone, "a", Some("a"), false), Keyed::Ignored);
}

#[test]
fn return_in_a_cards_box_sends_that_cards_text_only() {
    let a = row("qa", &[]);
    let b = row("qb", &[]);
    let mut drafts = Drafts::default();
    type_all(&mut drafts, &a, "first");
    type_all(&mut drafts, &b, "second");
    assert_eq!(drafts.typed("qa"), "first");
    assert_eq!(
        drafts.key(&b, "enter", Some("\r"), false),
        Keyed::Send(Some(Request::ask_answer("qb", AskAnswer::Text("second"))))
    );
    assert_eq!(drafts.typed("qa"), "first", "the other box is untouched");
    assert_eq!(drafts.key(&a, "backspace", None, false), Keyed::Changed);
    assert_eq!(drafts.typed("qa"), "firs");
    assert_eq!(drafts.key(&a, "v", Some("v"), true), Keyed::Ignored);
    assert_eq!(
        drafts.key(&row("qc", &[]), "enter", Some("\r"), false),
        Keyed::Send(None),
        "Return on an empty box does nothing"
    );
}

#[test]
fn drafts_of_cards_that_are_gone_are_dropped() {
    let a = row("qa", &[]);
    let mut drafts = Drafts::default();
    type_all(&mut drafts, &a, "text");
    drafts.settle("qa", Err("no".into()));
    drafts.keep_only(&["qb"]);
    assert_eq!(drafts.typed("qa"), "");
    assert_eq!(drafts.lost("qa"), None);
}

#[test]
fn drafts_of_other_sessions_survive_when_the_whole_feed_is_kept() {
    let mine = row("qa", &[]);
    let other = row("qb", &[]);
    let mut feed = crate::chat::asks::AskFeed::default();
    for r in ["qa", "qb"] {
        let wire = json!({
            "id": r, "session": if r == "qa" { "s1" } else { "s2" }, "asker": "main",
            "question": "Which?", "choices": [], "mainMayAnswer": false, "state": "open",
            "waiting": true, "createdMs": 1, "updatedMs": 1,
        });
        feed.apply("ask.changed", &json!({"row": wire}));
    }
    let mut drafts = Drafts::default();
    type_all(&mut drafts, &mine, "first");
    type_all(&mut drafts, &other, "second");
    drafts.act(&other, Act::Dismiss);
    let ids = feed.ids();
    let kept: Vec<&str> = ids.iter().map(String::as_str).collect();
    drafts.keep_only(&kept);
    assert_eq!(drafts.typed("qa"), "first");
    assert_eq!(drafts.typed("qb"), "second");
    assert!(drafts.busy("qb"), "a send in flight is not forgotten");
    feed.apply("ask.removed", &json!({"id": "qb", "session": "s2"}));
    let ids = feed.ids();
    let kept: Vec<&str> = ids.iter().map(String::as_str).collect();
    drafts.keep_only(&kept);
    assert_eq!(
        drafts.typed("qb"),
        "",
        "a question that left the feed is dropped"
    );
}
