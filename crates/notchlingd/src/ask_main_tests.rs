use crate::acting::ACTING_OFF;
use crate::ask_find_tests::{
    at, bash_line, lab, projects, put, subagent_file, Lab, A1, A2, EXE, MAIN,
};
use crate::ask_testkit::{asker_at, items, new_row, open_with, rig, until, ASKER, SHELL};
use crate::ask_waiters::ANY;
use crate::op_group::OpGroup;
use notchling_engine::ask::{AskState, Asker, By, Found};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

const SESSION: &str = crate::acting_ops_tests::SESSION;
const PERSON_ONLY: &str = "Only the person answers this question.";
const MAINCMD: u32 = 1020;
const OTHER_CLAUDE: u32 = 1050;
const OTHER_CMD: u32 = 1060;
const ANSWER_ID: &str = "q0123456789abcdef0123456789abcdef";

fn found(asker: Asker) -> Found {
    Found {
        asker,
        agent_id: None,
        label: None,
        agent_type: None,
        model: None,
        tool_use_id: None,
    }
}

fn answer_words() -> [&'static str; 5] {
    [EXE, "answer", ANSWER_ID, "--choice", "1"]
}

fn main_cmd(l: &Lab) {
    l.r.tree.child_of(MAINCMD, SHELL);
    l.r.tree.runs(MAINCMD, &answer_words());
}

fn asked(l: &Lab, flag: bool) -> String {
    let id = open_with(
        &l.r,
        json!({"question": "Which one?", "choices": ["a", "b"], "key": "k1", "mainMayAnswer": flag}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    until("found", || l.row(&id).asker == Asker::Subagent);
    l.r.clock
        .store(at("2026-11-16T10:04:05.000Z"), Ordering::SeqCst);
    id
}

fn answer(l: &Lab, pid: u32, id: &str) -> Result<Value, notchling_local_api::ApiError> {
    l.r.ops.call(
        &asker_at(pid),
        ops::ASK_ANSWER,
        &json!({"id": id, "choice": 1}),
    )
}

fn refused(result: Result<Value, notchling_local_api::ApiError>) {
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(error.message, PERSON_ONLY);
}

fn without_the_answer_line(l: &Lab) {
    let main: String = MAIN
        .lines()
        .filter(|line| !line.contains("toolu_main_answer"))
        .map(|line| format!("{line}\n"))
        .collect();
    put(projects(&l.r).join(format!("{SESSION}.jsonl")), &main);
}

#[test]
fn main_answers_its_subagents_question() {
    let l = lab(A1, A2);
    main_cmd(&l);
    let id = asked(&l, true);
    let reply = answer(&l, MAINCMD, &id).unwrap();
    assert_eq!(reply, json!({"answered": true}));
    let row = l.row(&id);
    assert_eq!(row.state, AskState::Answered);
    assert_eq!(row.by, Some(By::Main));
    assert_eq!(row.answer.as_deref(), Some("a"));
    assert_eq!(row.choice, Some(1));
}

#[test]
fn person_answering_first_wins() {
    let l = lab(A1, A2);
    main_cmd(&l);
    let id = asked(&l, true);
    let won =
        l.r.ops
            .call(
                &crate::acting_ops_tests::app(),
                ops::ASK_ANSWER,
                &json!({"id": id, "choice": 2}),
            )
            .unwrap();
    assert_eq!(won, json!({"answered": true}));
    let lost = answer(&l, MAINCMD, &id).unwrap();
    assert_eq!(
        lost,
        json!({"answered": false, "state": "answered", "by": "mac"})
    );
    assert_eq!(l.row(&id).answer.as_deref(), Some("b"));
}

#[test]
fn subagent_answer_is_refused() {
    let command = format!("{EXE} answer {ANSWER_ID} --choice 1");
    let a1 = format!(
        "{A1}{}",
        bash_line("2026-11-16T10:04:00.000Z", "toolu_a1_answer", &command)
    );
    let l = lab(&a1, A2);
    without_the_answer_line(&l);
    assert!(subagent_file(&l.r, "a1").is_file());
    main_cmd(&l);
    let id = asked(&l, true);
    refused(answer(&l, MAINCMD, &id));
    assert_eq!(l.row(&id).state, AskState::Open);
}

#[test]
fn other_session_is_refused() {
    let l = lab(A1, A2);
    main_cmd(&l);
    let id = asked(&l, true);
    std::fs::write(
        l.r.dir.0.join("claude/sessions/1050.json"),
        "{\"sessionId\":\"another-session\"}",
    )
    .unwrap();
    l.r.tree.child_of(OTHER_CLAUDE, 1);
    l.r.tree.child_of(OTHER_CMD, OTHER_CLAUDE);
    l.r.tree.runs(OTHER_CMD, &answer_words());
    refused(answer(&l, OTHER_CMD, &id));
    refused(answer(&l, crate::ask_testkit::STRANGER, &id));
    assert_eq!(l.row(&id).state, AskState::Open);
}

#[test]
fn main_cannot_answer_person_only() {
    let l = lab(A1, A2);
    main_cmd(&l);
    let id = asked(&l, false);
    refused(answer(&l, MAINCMD, &id));
    assert_eq!(l.row(&id).state, AskState::Open);
}

#[test]
fn main_cannot_answer_its_own_or_unknown() {
    let l = lab(A1, A2);
    main_cmd(&l);
    let id = asked(&l, true);
    for asker in [Asker::Main, Asker::Unknown] {
        l.r.store.set_found(&id, &found(asker), 1).unwrap();
        refused(answer(&l, MAINCMD, &id));
        assert_eq!(l.row(&id).state, AskState::Open);
    }
}

#[test]
fn an_answer_command_nobody_wrote_is_refused() {
    let l = lab(A1, A2);
    main_cmd(&l);
    l.r.tree
        .runs(MAINCMD, &[EXE, "answer", ANSWER_ID, "--choice", "2"]);
    let id = asked(&l, true);
    refused(answer(&l, MAINCMD, &id));
    assert_eq!(l.row(&id).state, AskState::Open);
}

#[test]
fn acting_off_refuses_the_main_answer() {
    let l = lab(A1, A2);
    main_cmd(&l);
    let id = asked(&l, true);
    l.r.acting.set(false);
    let error = answer(&l, MAINCMD, &id).unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(error.message, ACTING_OFF);
    assert_eq!(l.row(&id).state, AskState::Open);
}

fn seeded(
    r: &crate::ask_testkit::Rig,
    key: &str,
    session: &str,
    flag: bool,
    asker: Asker,
) -> String {
    let mut new = new_row(session, key, 70);
    new.main_may_answer = flag;
    let row = r.store.insert(&new, 1).unwrap();
    r.store.set_found(&row.id, &found(asker), 1).unwrap();
    row.id
}

fn mine(r: &crate::ask_testkit::Rig, args: Value) -> Vec<Value> {
    let list = r
        .ops
        .call(&asker_at(MAINCMD), ops::ASK_MINE, &args)
        .unwrap();
    list.as_array().unwrap().clone()
}

#[test]
fn mine_lists_only_answerable_without_answers() {
    let r = rig();
    r.tree.child_of(MAINCMD, SHELL);
    let yes = seeded(&r, "a", SESSION, true, Asker::Subagent);
    seeded(&r, "b", SESSION, false, Asker::Subagent);
    seeded(&r, "c", SESSION, true, Asker::Main);
    seeded(&r, "d", SESSION, true, Asker::Unknown);
    seeded(&r, "e", "another-session", true, Asker::Subagent);
    let closed = seeded(&r, "f", SESSION, true, Asker::Codex);
    r.store
        .close(&closed, &[AskState::Open], AskState::Dismissed, 2)
        .unwrap();
    let listed = mine(&r, json!({}));
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], json!(yes));
    assert_eq!(listed[0]["mainMayAnswer"], json!(true));
    assert!(listed[0].get("answer").is_none());
    assert!(mine(&r, json!({"waitMs": 30})).len() == 1);
    assert!(r
        .ops
        .call(&asker_at(ASKER), ops::ASK_MINE, &json!({}))
        .is_ok());
    let none = r.ops.call(
        &asker_at(crate::ask_testkit::STRANGER),
        ops::ASK_MINE,
        &json!({}),
    );
    assert_eq!(none.unwrap_err().code, ErrorCode::Forbidden);
}

#[test]
fn mine_without_one_returns_at_once_or_after_the_wait() {
    let r = rig();
    r.tree.child_of(MAINCMD, SHELL);
    assert!(mine(&r, json!({})).is_empty());
    let started = std::time::Instant::now();
    assert!(mine(&r, json!({"waitMs": 60})).is_empty());
    assert!(started.elapsed() >= std::time::Duration::from_millis(50));
}

#[test]
fn mine_waits_for_one() {
    let l = lab(A1, A2);
    main_cmd(&l);
    let ops_ = l.r.ops.clone();
    let waiting = std::thread::spawn(move || {
        ops_.call(&asker_at(MAINCMD), ops::ASK_MINE, &json!({"waitMs": 5000}))
            .unwrap()
    });
    until("waiter", || l.r.ops.waiters.connected(ANY));
    let id = asked(&l, true);
    let listed = waiting.join().unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], json!(id));
    assert_eq!(listed[0]["asker"], json!("subagent"));
    assert_eq!(items(&l.r).len(), 1);
}
