use super::*;
use crate::acting::AskAnswer;
use crate::chat::asks::AskFeed;
use crate::notch::answer::QuestionDraft;
use crate::notch::layout::NotchState;
use crate::notch::model::NotchModel;
use notchling_engine::sessions::{AgentGroup, AgentTarget};
use notchling_engine::transcript::snapshot::Snapshot;
use serde_json::json;
use std::collections::HashMap;

fn agent(job: &str, group: AgentGroup) -> Agent {
    Agent {
        id: format!("row-{job}"),
        name: job.into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group,
        label: "Working".into(),
        detail: None,
        since_ms: 0,
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
    }
}

fn wire(id: &str, job: &str, created: i64, state: &str, choices: &[&str]) -> serde_json::Value {
    json!({
        "id": id, "session": job, "asker": "subagent", "label": "Review",
        "question": format!("Question {id}?"), "choices": choices,
        "mainMayAnswer": false, "state": state, "waiting": true,
        "createdMs": created, "updatedMs": created,
    })
}

fn feed(rows: &[serde_json::Value]) -> AskFeed {
    let mut feed = AskFeed::default();
    for row in rows {
        feed.apply("ask.changed", &json!({"row": row}));
    }
    feed
}

fn model(agents: Vec<Agent>, feed: &AskFeed) -> NotchModel {
    let mut m = NotchModel::default();
    m.set_asks(feed);
    m.update(agents, HashMap::new(), HashMap::new());
    m
}

#[test]
fn notch_row_shows_question_while_open() {
    let open = feed(&[wire("q1", "j1", 1, "open", &["Keep it", "Drop it"])]);
    let m = model(
        vec![
            agent("j1", AgentGroup::Working),
            agent("j2", AgentGroup::Working),
        ],
        &open,
    );
    let asking = m.agent("row-j1").expect("the row");
    assert_eq!(asking.group, AgentGroup::NeedsInput);
    assert_eq!(m.status_line(asking), "Question");
    let quiet = m.agent("row-j2").expect("the row");
    assert_eq!(quiet.group, AgentGroup::Working);
    let card = m.question("row-j1").expect("a question card");
    assert_eq!(card.tool_use_id, "ask:q1");
    assert_eq!(card.items[0].question, "Question q1?");
    assert_eq!(card.items[0].options, ["Keep it", "Drop it"]);
    assert!(m.question("row-j2").is_none());

    let closed = feed(&[wire("q1", "j1", 1, "answered", &["Keep it", "Drop it"])]);
    let m = model(vec![agent("j1", AgentGroup::Working)], &closed);
    assert_eq!(
        m.agent("row-j1").map(|a| a.group),
        Some(AgentGroup::Working)
    );
    assert!(m.question("row-j1").is_none());
}

#[test]
fn an_answered_card_goes_at_once_and_a_new_question_peeks() {
    let open = feed(&[wire("q1", "j1", 1, "open", &[])]);
    let mut m = model(vec![agent("j1", AgentGroup::Working)], &open);
    m.mark_answered("row-j1");
    assert!(m.question("row-j1").is_none(), "answered here, gone now");
    let second = feed(&[
        wire("q1", "j1", 1, "open", &[]),
        wire("q2", "j1", 2, "open", &[]),
    ]);
    m.set_asks(&second);
    let peek = m.update(
        vec![agent("j1", AgentGroup::Working)],
        HashMap::new(),
        HashMap::new(),
    );
    assert_eq!(peek.as_deref(), Some("row-j1"));
    assert_eq!(
        m.question("row-j1").map(|q| q.tool_use_id),
        Some("ask:q2".into())
    );
    assert_ne!(m.state, NotchState::List);
}

#[test]
fn notch_card_answers_through_ask_answer() {
    let open = feed(&[wire("q1", "j1", 1, "open", &["Keep it", "Drop it"])]);
    let m = model(vec![agent("j1", AgentGroup::Working)], &open);
    let q = m.question("row-j1").expect("a card");
    let mut draft = QuestionDraft::new(&q);
    let answers = draft.choose(&q, 2).expect("one pick answers");
    assert_eq!(
        request_for("row-j1", &q, &answers),
        Request::ask_answer("q1", AskAnswer::Choice(2))
    );
    let mut draft = QuestionDraft::new(&q);
    draft.typed_mut().expect("a box").push_str("use the second");
    let answers = draft.submit_text(&q).expect("typed answers");
    assert_eq!(
        request_for("row-j1", &q, &answers),
        Request::ask_answer("q1", AskAnswer::Text("use the second"))
    );
    let other = PendingQuestion {
        tool_use_id: "tu1".into(),
        items: q.items.clone(),
    };
    let request = request_for("row-j1", &other, &[Answer::Options(vec![1])]);
    assert_eq!(request.op, "session.answer");
}

#[test]
fn notch_counts_one_of_n() {
    let three = feed(&[
        wire("q2", "j1", 20, "open", &[]),
        wire("q1", "j1", 10, "open", &[]),
        wire("q3", "j1", 30, "open", &[]),
    ]);
    let agents = vec![agent("j1", AgentGroup::Working)];
    let cards = cards(&agents, &three.open_rows());
    let card = &cards["row-j1"];
    assert_eq!(card.id, "q1", "the oldest first");
    assert_eq!((card.position, card.total), (1, 3));
    assert_eq!(card.step().as_deref(), Some("1 of 3"));

    let two = feed(&[
        wire("q2", "j1", 20, "open", &[]),
        wire("q3", "j1", 30, "open", &[]),
    ]);
    let card = &super::cards(&agents, &two.open_rows())["row-j1"];
    assert_eq!(card.id, "q2");
    assert_eq!(card.step().as_deref(), Some("1 of 2"));

    let one = feed(&[wire("q3", "j1", 30, "open", &[])]);
    let card = &super::cards(&agents, &one.open_rows())["row-j1"];
    assert_eq!(card.step(), None, "one question needs no count");
}

#[test]
fn a_lost_ask_answer_is_an_error_and_nothing_else_changes() {
    let lost = json!({"answered": false, "state": "answered", "by": "phone"});
    assert_eq!(
        lost_reply(Some("ask:q1"), &lost).as_deref(),
        Some("Already answered · Phone")
    );
    assert_eq!(lost_reply(Some("ask:q1"), &json!({"answered": true})), None);
    assert_eq!(lost_reply(Some("tu1"), &json!({"ok": true})), None);
    assert_eq!(lost_reply(None, &json!({"state": "answered"})), None);
    assert_eq!(lost_reply(Some("tu1"), &json!({"state": "answered"})), None);
    assert_eq!(
        lost_reply(None, &json!({"answered": false})).as_deref(),
        Some("Already answered")
    );
}

#[test]
fn an_ask_card_marks_its_own_key_not_an_answered_permission() {
    let open = feed(&[wire("q1", "j1", 1, "open", &[])]);
    let agents = vec![agent("j1", AgentGroup::Working)];
    let mut m = NotchModel::default();
    m.set_asks(&open);
    m.update(agents.clone(), HashMap::new(), HashMap::new());
    let hook = crate::notch::requests::HookRequest {
        id: "r1".into(),
        session_id: "row-j1".into(),
        created: 0.0,
        kind: crate::notch::requests::RequestKind::Permission,
        tool_name: "Bash".into(),
        summary: "ls".into(),
        description: String::new(),
        can_always_allow: false,
        agy: None,
    };
    m.update(
        agents,
        HashMap::new(),
        HashMap::from([("row-j1".to_owned(), hook)]),
    );
    m.mark_answered_key("r1");
    let q = m
        .question("row-j1")
        .expect("the ask shows once the permission is answered");
    assert_eq!(m.answer_key("row-j1").as_deref(), Some("r1"));
    assert_eq!(answer_key(&m, "row-j1", &q).as_deref(), Some("ask:q1"));
    let plain = PendingQuestion {
        tool_use_id: "tu1".into(),
        items: q.items.clone(),
    };
    assert_eq!(
        answer_key(&m, "row-j1", &plain),
        m.answer_key("row-j1"),
        "everything else is unchanged"
    );
}

fn hook_question(id: &str) -> crate::notch::requests::HookRequest {
    crate::notch::requests::HookRequest {
        id: id.into(),
        session_id: "row-j1".into(),
        created: 0.0,
        kind: crate::notch::requests::RequestKind::Question(PendingQuestion {
            tool_use_id: format!("hook:{id}"),
            items: vec![],
        }),
        tool_name: "AskUserQuestion".into(),
        summary: String::new(),
        description: String::new(),
        can_always_allow: false,
        agy: None,
    }
}

#[test]
fn an_answered_question_never_hides_an_ask_card() {
    let open = feed(&[wire("q1", "j1", 1, "open", &[])]);
    let agents = vec![agent("j1", AgentGroup::Working)];
    let mut m = NotchModel::default();
    m.set_asks(&open);
    m.update(
        agents.clone(),
        HashMap::new(),
        HashMap::from([("row-j1".to_owned(), hook_question("r1"))]),
    );
    assert_eq!(
        m.question("row-j1").map(|q| q.tool_use_id),
        Some("hook:r1".into()),
        "an open hook question still comes first"
    );
    m.mark_answered_key("r1");
    assert_eq!(
        m.question("row-j1").map(|q| q.tool_use_id),
        Some("ask:q1".into()),
        "answered hook question: the ask shows"
    );
    m.mark_answered_key("ask:q1");
    assert!(m.question("row-j1").is_none(), "both answered: nothing");

    let snap = Snapshot {
        question: Some(PendingQuestion {
            tool_use_id: "tu1".into(),
            items: vec![],
        }),
        ..Snapshot::default()
    };
    let mut m = NotchModel::default();
    m.set_asks(&open);
    m.update(
        agents,
        HashMap::from([("row-j1".to_owned(), snap)]),
        HashMap::new(),
    );
    m.mark_answered_key("tu1");
    assert_eq!(
        m.question("row-j1").map(|q| q.tool_use_id),
        Some("ask:q1".into()),
        "answered transcript question: the ask shows"
    );
}
