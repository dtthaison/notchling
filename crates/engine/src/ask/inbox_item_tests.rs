use super::*;
use crate::ask::model::{AskState, Asker, Mode, Row};
use crate::inbox::{ActionId, Kind, State};

fn row(asker: Asker, choices: Option<Vec<&str>>) -> Row {
    Row {
        n: 1,
        id: "q1".to_string(),
        key: "k".to_string(),
        session: "s1".to_string(),
        session_id: None,
        asker,
        agent_id: None,
        label: Some("Review Task 3".to_string()),
        agent_type: None,
        model: None,
        tool_use_id: None,
        folder: None,
        branch: Some("main".to_string()),
        question: "Keep the old API?".to_string(),
        choices: choices.map(|all| all.into_iter().map(str::to_string).collect()),
        main_may_answer: false,
        mode: Mode::Background,
        deadline_ms: None,
        pid: 1,
        pid_start: None,
        state: AskState::Open,
        answer: None,
        choice: None,
        by: None,
        inbox_id: None,
        created_ms: 0,
        updated_ms: 0,
        closed_ms: None,
    }
}

#[test]
fn inbox_item_shape() {
    let long = "d".repeat(80);
    let r = row(Asker::Main, Some(vec!["Keep it", "Drop it", &long]));
    let got = item(&r, None, 1_234);
    assert_eq!(got.id, "ask:q1:1234");
    assert_eq!(got.kind, Kind::Question);
    assert_eq!(got.title, "Review Task 3 asks · main");
    assert_eq!(got.detail, "Keep the old API?");
    assert_eq!(got.created_ms, 1_234);
    assert_eq!(got.expires_ms, None);
    assert_eq!(got.state, State::Open);
    assert_eq!(got.source.session.as_deref(), Some("s1"));
    assert_eq!(got.source.node, None);
    assert_eq!(
        got.open,
        Some(serde_json::json!({"ask": "q1", "session": "s1"}))
    );
    let ids: Vec<ActionId> = got.actions.iter().map(|a| a.id).collect();
    assert_eq!(
        ids,
        [
            ActionId::Choice1,
            ActionId::Choice2,
            ActionId::Choice3,
            ActionId::Open,
            ActionId::Cancel
        ]
    );
    assert_eq!(got.actions[0].label, "Keep it");
    assert_eq!(got.actions[2].label.chars().count(), CHOICE_LABEL_CHARS);
    assert_eq!(got.actions[3].label, "Answer in Chat");
    assert_eq!(got.actions[4].label, "Dismiss");
    assert!(got.actions.iter().all(|a| !a.primary));
}

#[test]
fn inbox_item_without_choices_makes_open_primary() {
    let got = item(&row(Asker::Subagent, None), None, 5);
    let ids: Vec<(ActionId, bool)> = got.actions.iter().map(|a| (a.id, a.primary)).collect();
    assert_eq!(ids, [(ActionId::Open, true), (ActionId::Cancel, false)]);
}

#[test]
fn inbox_item_names_the_asker() {
    let mut unknown = row(Asker::Unknown, None);
    unknown.label = None;
    assert_eq!(
        item(&unknown, None, 1).title,
        "This session or one of its subagents asks · main"
    );
    let mut no_branch = row(Asker::Main, None);
    no_branch.branch = None;
    assert_eq!(item(&no_branch, None, 1).title, "Review Task 3 asks");
}

#[test]
fn inbox_item_on_a_worker_ends_with_the_node() {
    let got = item(&row(Asker::Subagent, None), Some("m1"), 1);
    assert_eq!(got.title, "Review Task 3 asks · main on m1");
    assert_eq!(got.source.node.as_deref(), Some("m1"));
    let mut long = row(Asker::Subagent, None);
    long.label = Some("l".repeat(200));
    let cut_title = item(&long, Some("m1"), 1).title;
    assert_eq!(cut_title.chars().count(), TITLE_CHARS);
    assert!(cut_title.ends_with(" on m1"));
}

#[test]
fn inbox_title_is_cut_to_eighty() {
    let mut long = row(Asker::Subagent, None);
    long.label = Some("l".repeat(200));
    assert_eq!(item(&long, None, 1).title.chars().count(), TITLE_CHARS);
}

#[test]
fn inbox_detail_cuts_on_a_char_boundary() {
    let mut r = row(Asker::Main, None);
    r.question = format!("{}😀", "a".repeat(399));
    assert_eq!(item(&r, None, 1).detail.chars().count(), 400);
    r.question = format!("{}😀tail", "a".repeat(399));
    let got = item(&r, None, 1).detail;
    assert_eq!(got.chars().count(), DETAIL_CHARS);
    assert!(got.ends_with('…'));
}

#[test]
fn choice_actions_round_trip_as_choice_n() {
    assert_eq!(
        serde_json::to_value(ActionId::Choice1).unwrap(),
        serde_json::json!("choice_1")
    );
    for n in 1..=6u8 {
        let word = format!("choice_{n}");
        assert_eq!(
            serde_json::to_value(action_for(n)).unwrap(),
            serde_json::json!(word)
        );
        assert_eq!(
            serde_json::from_value::<ActionId>(serde_json::json!(word)).unwrap(),
            action_for(n)
        );
        assert_eq!(choice_of(action_for(n)), Some(n));
    }
    assert_eq!(choice_of(ActionId::Open), None);
    assert_eq!(choice_of(ActionId::Cancel), None);
    assert_eq!(
        serde_json::from_value::<Kind>(serde_json::json!("question")).unwrap(),
        Kind::Question
    );
}
