use crate::acting::ACTING_OFF;
use crate::acting_ops_tests::{code, sessions, SESSION};
use crate::ask_ops::Taken;
use crate::ask_testkit::{
    asker_at, base, changes, new_row, open, open_with, person, rig, rig_with, scratch, ASKER,
    STRANGER, T0,
};
use crate::event_assert::{of, text};
use crate::op_group::OpGroup;
use notchling_engine::ask::{AskState, AskStore, Asker, By, Mode};
use notchling_local_api::{ops, topics, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

const PERSON_ONLY: &str = "Only the person answers this question.";
const NO_SESSION: &str = "Notchling can't tell which session is asking.";

#[test]
fn open_saves_and_publishes() {
    let r = rig();
    let opened = open_with(&r, base()).unwrap();
    let id = opened["id"].as_str().unwrap().to_string();
    assert_eq!(opened["state"], json!("open"));
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!(row.state, AskState::Open);
    assert_eq!(row.session, SESSION);
    assert_eq!(row.session_id.as_deref(), Some(SESSION));
    assert_eq!(row.pid, ASKER);
    assert_eq!(row.pid_start, Some(4242));
    assert_eq!(row.branch.as_deref(), Some("agent/ask"));
    assert!(row.folder.unwrap().ends_with("work"));
    assert_eq!(row.asker, Asker::Unknown);
    assert_eq!(row.mode, Mode::Background);
    assert_eq!(row.created_ms, T0);
    let changed = changes(&r);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["id"], json!(id));
    assert_eq!(changed[0]["state"], json!("open"));
    assert_eq!(changed[0]["waiting"], json!(false));
    let asked = of(&r.sink, "session.ask");
    assert_eq!(asked.len(), 1);
    assert_eq!(text(&asked[0], "then").as_deref(), Some("asked"));
    assert_eq!(text(&asked[0], "chars").as_deref(), Some("10"));
    assert_eq!(text(&asked[0], "choices").as_deref(), Some("2"));
    assert_eq!(text(&asked[0], "mode").as_deref(), Some("background"));
}

#[test]
fn open_reads_the_mode_and_deadline_from_its_arguments() {
    let r = rig();
    let mut args = base();
    args["wait"] = json!(true);
    args["deadlineMs"] = json!(T0 + 60_000);
    args["mainMayAnswer"] = json!(true);
    let id = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!(row.mode, Mode::Wait);
    assert_eq!(row.deadline_ms, Some(T0 + 60_000));
    assert!(row.main_may_answer);
}

#[test]
fn a_retried_open_returns_the_same_question_and_says_nothing_twice() {
    let r = rig();
    let first = open(&r);
    let again = open(&r);
    assert_eq!(first, again);
    assert_eq!(r.store.list(None).unwrap().len(), 1);
    assert_eq!(changes(&r).len(), 1);
    assert_eq!(of(&r.sink, "session.ask").len(), 1);
}

#[test]
fn open_refused_outside_a_session() {
    let r = rig();
    let refused = r.ops.call(&asker_at(STRANGER), ops::ASK_OPEN, &base());
    assert_eq!(
        code(refused),
        (ErrorCode::Forbidden, NO_SESSION.to_string())
    );
    let no_pid = Caller {
        pid: None,
        ..asker_at(ASKER)
    };
    assert_eq!(
        code(r.ops.call(&no_pid, ops::ASK_OPEN, &base())),
        (ErrorCode::Forbidden, NO_SESSION.to_string())
    );
    assert!(r.store.list(None).unwrap().is_empty());
    assert!(changes(&r).is_empty());
    let events = of(&r.sink, "session.ask");
    assert_eq!(events.len(), 2);
    for event in &events {
        assert_eq!(text(event, "then").as_deref(), Some("refused"));
    }
}

#[test]
fn open_refusals_have_exact_words() {
    let r = rig();
    let with = |question: &str, choices: Value, key: &str| {
        let mut args = json!({"question": question, "key": key});
        if !choices.is_null() {
            args["choices"] = choices;
        }
        code(open_with(&r, args))
    };
    let bad = ErrorCode::BadRequest;
    assert_eq!(
        with("", Value::Null, "e1"),
        (bad, "Ask a question first.".to_string())
    );
    assert_eq!(
        with("  ", Value::Null, "e2"),
        (bad, "Ask a question first.".to_string())
    );
    assert_eq!(
        with(&"x".repeat(2_001), Value::Null, "e3"),
        (bad, "That question is over 2,000 characters.".to_string())
    );
    assert_eq!(
        with("q", json!(["only"]), "e4"),
        (bad, "Give 2 to 6 choices.".to_string())
    );
    assert_eq!(
        with("q", json!(["a", "b", "c", "d", "e", "f", "g"]), "e5"),
        (bad, "Give 2 to 6 choices.".to_string())
    );
    assert_eq!(
        with("q", json!("a"), "e6"),
        (bad, "Give 2 to 6 choices.".to_string())
    );
    assert_eq!(
        with("q", json!(["a", ""]), "e7"),
        (bad, "A choice is empty or over 120 characters.".to_string())
    );
    assert_eq!(
        with("q", json!(["a", "b".repeat(121)]), "e8"),
        (bad, "A choice is empty or over 120 characters.".to_string())
    );
    assert!(r.store.list(None).unwrap().is_empty());
    for n in 0..20 {
        let args = json!({"question": "q", "key": format!("fill{n}")});
        open_with(&r, args).unwrap();
    }
    assert_eq!(
        with("q", Value::Null, "over-session"),
        (
            ErrorCode::Failed,
            "20 questions from this session are already waiting.".to_string()
        )
    );
}

#[test]
fn the_hundred_and_first_question_is_refused() {
    let r = rig();
    for n in 0..100 {
        let session = format!("other{}", n % 5);
        r.store
            .insert(&new_row(&session, &format!("k{n}"), 9000 + n), T0)
            .unwrap();
    }
    assert_eq!(
        code(open_with(&r, base())),
        (
            ErrorCode::Failed,
            "100 questions are already waiting.".to_string()
        )
    );
}

#[test]
fn a_store_that_cannot_save_says_so() {
    let r = rig();
    let ops = Arc::try_unwrap(r.ops).ok().unwrap().without_store();
    let refused = ops.call(&asker_at(ASKER), ops::ASK_OPEN, &base());
    assert_eq!(
        code(refused),
        (ErrorCode::Failed, "Couldn't save the question.".to_string())
    );
}

#[test]
fn acting_gate() {
    let r = rig();
    let id = open(&r);
    r.acting.set(false);
    let off = (ErrorCode::Forbidden, ACTING_OFF.to_string());
    let mut args = base();
    args["key"] = json!("k2");
    assert_eq!(code(open_with(&r, args)), off);
    assert_eq!(
        code(person(&r, ops::ASK_ANSWER, json!({"id": id, "choice": 1}))),
        off
    );
    assert_eq!(code(person(&r, ops::ASK_DISMISS, json!({"id": id}))), off);
    assert_eq!(
        code(r.ops.call(
            &asker_at(ASKER),
            ops::ASK_ANSWER,
            &json!({"id": id, "choice": 1})
        )),
        off
    );
    let listed = person(&r, ops::ASK_LIST, json!({})).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, AskState::Open);
}

#[test]
fn person_answers_and_dismisses() {
    let r = rig();
    let first = open(&r);
    r.clock.store(T0 + 5_000, Ordering::SeqCst);
    let answered = person(&r, ops::ASK_ANSWER, json!({"id": first, "choice": 1})).unwrap();
    assert_eq!(answered, json!({"answered": true}));
    let row = r.store.get(&first).unwrap().unwrap();
    assert_eq!(row.state, AskState::Answered);
    assert_eq!(row.by, Some(By::Mac));
    assert_eq!(row.choice, Some(1));
    assert_eq!(row.answer.as_deref(), Some("a"));
    let mut args = base();
    args["key"] = json!("k2");
    let second = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let typed = person(
        &r,
        ops::ASK_ANSWER,
        json!({"id": second, "text": "something else"}),
    );
    assert_eq!(typed.unwrap(), json!({"answered": true}));
    assert_eq!(
        r.store.get(&second).unwrap().unwrap().answer.as_deref(),
        Some("something else")
    );
    let mut args = base();
    args["key"] = json!("k3");
    let third = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let dismissed = person(&r, ops::ASK_DISMISS, json!({"id": third})).unwrap();
    assert_eq!(dismissed, json!({"dismissed": true}));
    let row = r.store.get(&third).unwrap().unwrap();
    assert_eq!(row.state, AskState::Dismissed);
    assert!(row.closed_ms.is_some());
    let again = person(&r, ops::ASK_DISMISS, json!({"id": third})).unwrap();
    assert_eq!(again, json!({"dismissed": false, "state": "dismissed"}));
    let late = person(&r, ops::ASK_ANSWER, json!({"id": third, "choice": 2})).unwrap();
    assert_eq!(late, json!({"answered": false, "state": "dismissed"}));
    let then: Vec<String> = of(&r.sink, "session.ask")
        .iter()
        .filter_map(|e| text(e, "then"))
        .collect();
    assert_eq!(
        then,
        [
            "asked",
            "answered",
            "asked",
            "answered",
            "asked",
            "dismissed"
        ]
    );
    let answered = &of(&r.sink, "session.ask")[1];
    assert_eq!(text(answered, "by").as_deref(), Some("mac"));
    assert_eq!(text(answered, "answer_chars").as_deref(), Some("1"));
    assert_eq!(text(answered, "waited").as_deref(), Some("5000"));
}

#[test]
fn answer_refusals_have_exact_words() {
    let r = rig();
    let id = open(&r);
    let bad = ErrorCode::BadRequest;
    let call = |args: Value| code(person(&r, ops::ASK_ANSWER, args));
    assert_eq!(
        call(json!({"id": id})),
        (bad, "Answer with a choice or text.".to_string())
    );
    assert_eq!(
        call(json!({"id": id, "choice": 1, "text": "x"})),
        (bad, "Answer with a choice or text.".to_string())
    );
    assert_eq!(
        call(json!({"id": id, "text": "  "})),
        (bad, "Answer with a choice or text.".to_string())
    );
    assert_eq!(
        call(json!({"id": id, "text": "x".repeat(10 * 1024 + 1)})),
        (bad, "That answer is over 10 KB.".to_string())
    );
    assert_eq!(
        call(json!({"id": "q-nothing", "choice": 1})),
        (bad, "No such question.".to_string())
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, AskState::Open);
}

#[test]
fn an_agent_cannot_answer_until_the_main_session_may() {
    let r = rig();
    let id = open(&r);
    let refused = r.ops.call(
        &asker_at(ASKER),
        ops::ASK_ANSWER,
        &json!({"id": id, "choice": 1}),
    );
    assert_eq!(
        code(refused),
        (ErrorCode::Forbidden, PERSON_ONLY.to_string())
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, AskState::Open);
}

#[test]
fn first_answer_wins_under_race() {
    let r = rig();
    let mut wins = (0, 0);
    for round in 0..1_000 {
        let mut row = new_row(&format!("s{round}"), &format!("k{round}"), 5000);
        row.question = "pick".into();
        let id = r.store.insert(&row, T0).unwrap().id;
        let (mac, phone) = std::thread::scope(|scope| {
            let a = scope.spawn(|| r.ops.answer(&id, Some("from-mac"), None, By::Mac));
            let b = scope.spawn(|| r.ops.answer(&id, Some("from-phone"), None, By::Phone));
            (a.join().unwrap().unwrap(), b.join().unwrap().unwrap())
        });
        let stored = r.store.get(&id).unwrap().unwrap();
        match (mac, phone) {
            (Taken::Won, Taken::Lost { state, by }) => {
                wins.0 += 1;
                assert_eq!((state, by), (AskState::Answered, Some(By::Mac)));
                assert_eq!(stored.by, Some(By::Mac));
                assert_eq!(stored.answer.as_deref(), Some("from-mac"));
            }
            (Taken::Lost { state, by }, Taken::Won) => {
                wins.1 += 1;
                assert_eq!((state, by), (AskState::Answered, Some(By::Phone)));
                assert_eq!(stored.by, Some(By::Phone));
                assert_eq!(stored.answer.as_deref(), Some("from-phone"));
            }
            other => panic!("round {round}: {other:?}"),
        }
        r.store
            .close(&id, &[AskState::Answered], AskState::Delivered, T0)
            .unwrap();
    }
    assert_eq!(wins.0 + wins.1, 1_000);
    let answered = of(&r.sink, "session.ask")
        .iter()
        .filter(|e| text(e, "then").as_deref() == Some("answered"))
        .count();
    assert_eq!(answered, 1_000);
}

#[test]
fn failed_write_leaves_it_open() {
    let dir = scratch("failing");
    let data = dir.0.join("data");
    let store = Arc::new(AskStore::open(&data).unwrap());
    let r = rig_with(json!([]), Some((store, dir)));
    let id = open(&r);
    let file = notchling_engine::store::db_path(&r.dir.0.join("data"));
    let side = rusqlite::Connection::open(file).unwrap();
    side.execute_batch(
        "CREATE TRIGGER nothing_saves BEFORE UPDATE ON asks
         BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
    )
    .unwrap();
    let failed = person(&r, ops::ASK_ANSWER, json!({"id": id, "choice": 1}));
    assert_eq!(
        code(failed),
        (ErrorCode::Failed, "Couldn't save the answer.".to_string())
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, AskState::Open);
    assert!(of(&r.sink, "session.ask")
        .iter()
        .all(|e| text(e, "then").as_deref() != Some("answered")));
    side.execute_batch("DROP TRIGGER nothing_saves").unwrap();
    let won = person(&r, ops::ASK_ANSWER, json!({"id": id, "choice": 2})).unwrap();
    assert_eq!(won, json!({"answered": true}));
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!(
        (row.state, row.answer.as_deref()),
        (AskState::Answered, Some("b"))
    );
}

#[test]
fn list_and_topic_rows_have_answers_agents_never_do() {
    let r = rig();
    let id = open(&r);
    person(&r, ops::ASK_ANSWER, json!({"id": id, "choice": 2})).unwrap();
    let listed = person(&r, ops::ASK_LIST, json!({})).unwrap();
    assert_eq!(listed[0]["answer"], json!("b"));
    assert_eq!(listed[0]["by"], json!("mac"));
    assert_eq!(listed[0]["state"], json!("answered"));
    let topic = changes(&r);
    assert_eq!(topic.last().unwrap()["answer"], json!("b"));
    assert_eq!(r.ops.spec(ops::ASK_LIST), Some(OpSpec::APP));
    assert_eq!(r.ops.spec(ops::ASK_DISMISS), Some(OpSpec::APP));
    assert_eq!(r.ops.spec(ops::ASK_OPEN), Some(OpSpec::AGENT_READ));
    assert_eq!(r.ops.spec(ops::ASK_ANSWER), Some(OpSpec::AGENT_READ));
    assert_eq!(r.ops.topic_tier(topics::ASKS), Some(Tier::App));
    assert_eq!(r.ops.topics(), &[topics::ASKS]);
    let by_agent = r.ops.call(&asker_at(ASKER), ops::ASK_LIST, &json!({}));
    assert_eq!(code(by_agent).0, ErrorCode::Forbidden);
    let by_agent = r
        .ops
        .call(&asker_at(ASKER), ops::ASK_DISMISS, &json!({"id": id}));
    assert_eq!(code(by_agent).0, ErrorCode::Forbidden);
}

#[test]
fn list_filters_by_session_and_keeps_order() {
    let r = rig_with(sessions(), None);
    let first = open(&r);
    r.store
        .insert(&new_row("elsewhere", "other", 7), T0)
        .unwrap();
    let mut args = base();
    args["key"] = json!("k2");
    let second = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let all = person(&r, ops::ASK_LIST, json!({})).unwrap();
    assert_eq!(all.as_array().unwrap().len(), 3);
    for id in ["job-a", "abcd1234"] {
        let mine = person(&r, ops::ASK_LIST, json!({"session": id})).unwrap();
        let ids: Vec<&str> = mine
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, [first.as_str(), second.as_str()], "{id}");
    }
}

#[test]
fn session_key_uses_the_job_short_id_then_rekeys() {
    let r = rig();
    let id = open(&r);
    assert_eq!(r.store.get(&id).unwrap().unwrap().session, SESSION);
    *r.list.lock().unwrap() = sessions();
    let before = changes(&r).len();
    r.ops.sessions_changed();
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!(row.session, "abcd1234");
    assert_eq!(row.session_id.as_deref(), Some(SESSION));
    let changed = changes(&r);
    assert_eq!(changed.len(), before + 1);
    assert_eq!(changed.last().unwrap()["session"], json!("abcd1234"));
    r.ops.sessions_changed();
    assert_eq!(changes(&r).len(), before + 1);
    let mut args = base();
    args["key"] = json!("later");
    let later = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(r.store.get(&later).unwrap().unwrap().session, "abcd1234");
}

#[test]
fn no_text_in_events_or_audit() {
    const TOKEN: &str = "PLANTED-9f3a71c2";
    let r = rig();
    let args = json!({
        "question": format!("Is {TOKEN} fine?"),
        "choices": [format!("yes {TOKEN}"), "no"],
        "key": "plant",
    });
    let id = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    person(
        &r,
        ops::ASK_ANSWER,
        json!({"id": id, "text": format!("answer {TOKEN}")}),
    )
    .unwrap();
    let mut args = base();
    args["key"] = json!("second");
    let second = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    person(
        &r,
        ops::ASK_ANSWER,
        json!({"id": second, "text": format!("typed {TOKEN}")}),
    )
    .unwrap();
    person(&r, ops::ASK_ANSWER, json!({"id": second, "text": TOKEN})).unwrap();
    let mut args = base();
    args["key"] = json!("third");
    args["question"] = json!(TOKEN);
    let third = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    person(&r, ops::ASK_DISMISS, json!({"id": third})).unwrap();
    let events = r.sink.events();
    assert!(events.len() >= 6, "{}", events.len());
    for event in &events {
        assert!(!format!("{event:?}").contains(TOKEN), "{event:?}");
        for field in event.fields.names() {
            assert_ne!(field, "question");
            assert_ne!(field, "answer");
        }
    }
    let topic = r.room.events.lock().unwrap().clone();
    assert!(format!("{topic:?}").contains(TOKEN));
}
