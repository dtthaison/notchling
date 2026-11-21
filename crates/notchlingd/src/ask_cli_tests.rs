use super::*;
use notchling_local_api::ErrorCode;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

pub(crate) const T0: i64 = 1_794_697_060_000;

pub(crate) fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

pub(crate) struct Fake {
    pub(crate) script: VecDeque<Result<Value, LinkError>>,
    pub(crate) calls: Vec<(String, Value)>,
    pub(crate) reconnects: usize,
    pub(crate) clock: Rc<Cell<i64>>,
    pub(crate) idle_waits: bool,
}

impl Fake {
    pub(crate) fn new(clock: &Rc<Cell<i64>>, script: Vec<Result<Value, LinkError>>) -> Fake {
        Fake {
            script: script.into(),
            calls: Vec::new(),
            reconnects: 0,
            clock: clock.clone(),
            idle_waits: false,
        }
    }

    pub(crate) fn ops(&self) -> Vec<&str> {
        self.calls.iter().map(|(op, _)| op.as_str()).collect()
    }

    pub(crate) fn of(&self, op: &str) -> Vec<&Value> {
        self.calls
            .iter()
            .filter(|(o, _)| o == op)
            .map(|(_, a)| a)
            .collect()
    }
}

impl AskLink for Fake {
    fn call(&mut self, op: &str, args: &Value, _timeout: Duration) -> Result<Value, LinkError> {
        self.calls.push((op.to_string(), args.clone()));
        if let Some(next) = self.script.pop_front() {
            return next;
        }
        if self.idle_waits && op == "ask.wait" {
            let ms = args["waitMs"].as_i64().unwrap();
            self.clock.set(self.clock.get() + ms);
            return Ok(json!({"state": "open"}));
        }
        Err(LinkError::Down)
    }

    fn reconnect(&mut self) -> Result<(), LinkError> {
        self.reconnects += 1;
        Ok(())
    }
}

pub(crate) struct Rig {
    pub(crate) clock: Rc<Cell<i64>>,
    pub(crate) sleeps: Rc<RefCell<Vec<u64>>>,
    pub(crate) out: Vec<u8>,
    pub(crate) err: Vec<u8>,
    pub(crate) delegate: Option<i64>,
}

impl Rig {
    pub(crate) fn new() -> Rig {
        Rig {
            clock: Rc::new(Cell::new(T0)),
            sleeps: Rc::new(RefCell::new(Vec::new())),
            out: Vec::new(),
            err: Vec::new(),
            delegate: None,
        }
    }

    pub(crate) fn run<R>(&mut self, f: impl FnOnce(&mut Env) -> R) -> R {
        let clock = self.clock.clone();
        let now = move || clock.get();
        let clock = self.clock.clone();
        let sleeps = self.sleeps.clone();
        let sleep = move |ms: u64| {
            sleeps.borrow_mut().push(ms);
            clock.set(clock.get() + ms as i64);
        };
        let mut env = Env {
            now: &now,
            sleep: &sleep,
            delegate_deadline: self.delegate,
            key: "0123456789abcdef0123456789abcdef".to_string(),
            out: &mut self.out,
            err: &mut self.err,
        };
        f(&mut env)
    }

    pub(crate) fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.out).into_owned()
    }

    pub(crate) fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.err).into_owned()
    }
}

pub(crate) fn opened(id: &str) -> Result<Value, LinkError> {
    Ok(json!({"id": id, "state": "open"}))
}

pub(crate) fn refusal(code: ErrorCode, message: &str) -> Result<Value, LinkError> {
    Err(LinkError::Api(ApiError::new(code, message)))
}

pub(crate) fn ask_args(list: &[&str]) -> AskArgs {
    parse_ask(&words(list)).unwrap()
}

#[test]
fn backoff_steps_then_holds_at_ten_seconds() {
    let mut backoff = Backoff::new();
    let steps: Vec<u64> = (0..7).map(|_| backoff.next_ms()).collect();
    assert_eq!(steps, [1_000, 2_000, 5_000, 10_000, 10_000, 10_000, 10_000]);
}

#[test]
fn prints_answer_and_from() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![
            opened("q1"),
            Ok(json!({"state": "answered", "answer": "Keep it", "by": "phone"})),
        ],
    );
    let code = rig.run(|env| ask(&mut link, &ask_args(&["--", "Keep it?"]), env));
    assert_eq!(code, 0);
    assert_eq!(rig.stdout(), "Answer: Keep it\nFrom: you (phone)\n");
    assert_eq!(rig.stderr(), "Asked (q1). Waiting for the answer.\n");
    assert_eq!(link.ops(), ["ask.open", "ask.wait"]);
    assert_eq!(
        link.of("ask.wait")[0],
        &json!({"id": "q1", "waitMs": 50_000})
    );
    let open = link.of("ask.open")[0];
    assert_eq!(open["question"], "Keep it?");
    assert_eq!(open["key"], "0123456789abcdef0123456789abcdef");
    assert!(open.get("wait").is_none() && open.get("deadlineMs").is_none());
}

#[test]
fn from_names_each_answerer() {
    for (by, line) in [
        ("mac", "From: you (Mac)"),
        ("phone", "From: you (phone)"),
        ("main", "From: the main session"),
    ] {
        let mut rig = Rig::new();
        let mut link = Fake::new(
            &rig.clock,
            vec![
                opened("q1"),
                Ok(json!({"state": "answered", "answer": "x", "choice": 2, "by": by})),
            ],
        );
        assert_eq!(
            rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
            0
        );
        assert!(rig.stdout().ends_with(&format!("{line}\n")), "{by}");
    }
}

#[test]
fn json_prints_the_answer_record() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![
            opened("q9"),
            Ok(json!({"state": "answered", "answer": "Keep it", "choice": 1, "by": "mac"})),
        ],
    );
    let args = ask_args(&[
        "--json", "--choice", "Keep it", "--choice", "Drop it", "--", "Which?",
    ]);
    assert_eq!(rig.run(|env| ask(&mut link, &args, env)), 0);
    let printed: Value = serde_json::from_str(rig.stdout().trim()).unwrap();
    assert_eq!(
        printed,
        json!({"id": "q9", "state": "answered", "answer": "Keep it", "choice": 1, "by": "mac"})
    );
    assert_eq!(rig.stderr(), "");
    let open = link.of("ask.open")[0];
    assert_eq!(open["choices"], json!(["Keep it", "Drop it"]));
}

#[test]
fn json_keeps_the_keys_when_there_is_no_choice() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![
            opened("q9"),
            Ok(json!({"state": "answered", "answer": "free text", "by": "mac"})),
        ],
    );
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--json", "--", "q"]), env)),
        0
    );
    let printed: Value = serde_json::from_str(rig.stdout().trim()).unwrap();
    assert_eq!(printed["choice"], Value::Null);
    assert_eq!(printed["answer"], "free text");
}

#[test]
fn reconnects_with_the_same_id() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![
            opened("q1"),
            Err(LinkError::Down),
            Err(LinkError::Down),
            Ok(json!({"state": "answered", "answer": "Yes", "by": "mac"})),
        ],
    );
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
        0
    );
    assert_eq!(*rig.sleeps.borrow(), [1_000, 2_000]);
    assert_eq!(link.reconnects, 2);
    let waits = link.of("ask.wait");
    assert_eq!(waits.len(), 3);
    assert!(waits.iter().all(|w| w["id"] == "q1"));
    assert_eq!(link.of("ask.open").len(), 1, "nothing is asked twice");
    assert_eq!(rig.stdout(), "Answer: Yes\nFrom: you (Mac)\n");
}

#[test]
fn backoff_restarts_after_a_reply() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![
            opened("q1"),
            Err(LinkError::Down),
            Ok(json!({"state": "open"})),
            Err(LinkError::Down),
            Ok(json!({"state": "answered", "answer": "Yes", "by": "mac"})),
        ],
    );
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
        0
    );
    assert_eq!(*rig.sleeps.borrow(), [1_000, 1_000]);
}

#[test]
fn retried_open_sends_the_same_key() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![
            Err(LinkError::Down),
            Err(LinkError::Down),
            opened("q1"),
            Ok(json!({"state": "answered", "answer": "Yes", "by": "mac"})),
        ],
    );
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
        0
    );
    let opens = link.of("ask.open");
    assert_eq!(opens.len(), 3);
    assert!(opens.iter().all(|o| o["key"] == opens[0]["key"]));
    assert!(opens.iter().all(|o| o["question"] == "q"));
    assert_eq!(*rig.sleeps.borrow(), [1_000, 2_000]);
}

#[test]
fn dismissed_exits_5() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![opened("q1"), Ok(json!({"state": "dismissed"}))],
    );
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
        5
    );
    assert!(rig
        .stderr()
        .contains("No answer: the question was dismissed. (denied)\n"));
    assert_eq!(rig.stdout(), "");
}

#[test]
fn a_refusal_exits_by_its_kind_with_the_reason() {
    for (code, message) in [
        (
            ErrorCode::Forbidden,
            "Notchling can't tell which session is asking.",
        ),
        (
            ErrorCode::Forbidden,
            "acting is off: the Swift app owns sessions until the maintainer turns this on",
        ),
        (
            ErrorCode::Failed,
            "20 questions from this session are already waiting.",
        ),
        (
            ErrorCode::BadRequest,
            "On a Worker, give choices (--choice): the answer comes from your Mac's Inbox.",
        ),
    ] {
        let mut rig = Rig::new();
        let mut link = Fake::new(&rig.clock, vec![refusal(code, message)]);
        let exit = match code {
            ErrorCode::Forbidden => 4,
            ErrorCode::BadRequest => 2,
            _ => 1,
        };
        assert_eq!(
            rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
            exit
        );
        let name = crate::contract::name_of(exit);
        assert_eq!(rig.stderr(), format!("{message} ({name})\n"));
        assert_eq!(rig.stdout(), "");
        assert_eq!(link.ops(), ["ask.open"], "a refusal is not retried");
    }
}

#[test]
fn no_daemon_for_60s_exits_3() {
    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![]);
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
        3
    );
    assert_eq!(rig.sleeps.borrow().iter().sum::<u64>(), 60_000);
    assert_eq!(&rig.sleeps.borrow()[..4], [1_000, 2_000, 5_000, 10_000]);
    assert_eq!(
        rig.stderr(),
        "Notchling isn't running. Ask the user to open it, then try again. (not_running)\n"
    );
    let opens = link.of("ask.open");
    assert!(opens.len() > 3);
    assert!(opens.iter().all(|o| o["key"] == opens[0]["key"]));
}

#[test]
fn a_wait_that_is_refused_exits_1() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![
            opened("q1"),
            refusal(ErrorCode::BadRequest, "No such question."),
        ],
    );
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
        1
    );
    assert!(rig.stderr().ends_with("No such question.\n"));
}

#[test]
fn wait_deadline_is_capped_by_the_delegate() {
    let mut rig = Rig::new();
    rig.delegate = Some(T0 + 600_000);
    let mut link = Fake::new(&rig.clock, vec![opened("q1")]);
    link.idle_waits = true;
    let args = ask_args(&["--wait", "--timeout", "3000", "--", "q"]);
    assert_eq!(rig.run(|env| ask(&mut link, &args, env)), 124);
    let open = link.of("ask.open")[0];
    assert_eq!(open["deadlineMs"], json!(T0 + 540_000));
    assert_eq!(open["wait"], json!(true));
    assert_eq!(rig.stderr().lines().last(), Some("No answer in time. (expired)"));
    assert_eq!(rig.stdout(), "");
    let waits = link.of("ask.wait");
    assert!(waits
        .iter()
        .all(|w| w["waitMs"].as_i64().unwrap() <= 50_000));
    assert_eq!(
        rig.clock.get(),
        T0 + 540_000,
        "stops at the deadline, not later"
    );
}

#[test]
fn wait_deadline_defaults_to_900_seconds_and_follows_timeout() {
    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![opened("q1")]);
    link.idle_waits = true;
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--wait", "--", "q"]), env)),
        124
    );
    assert_eq!(link.of("ask.open")[0]["deadlineMs"], json!(T0 + 900_000));
    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![opened("q1")]);
    link.idle_waits = true;
    let args = ask_args(&["--wait", "--timeout", "120", "--", "q"]);
    assert_eq!(rig.run(|env| ask(&mut link, &args, env)), 124);
    assert_eq!(link.of("ask.open")[0]["deadlineMs"], json!(T0 + 120_000));
    let waits: Vec<i64> = link
        .of("ask.wait")
        .iter()
        .map(|w| w["waitMs"].as_i64().unwrap())
        .collect();
    assert_eq!(waits, [50_000, 50_000, 20_000]);
}

#[test]
fn the_daemon_saying_expired_exits_124() {
    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![opened("q1"), Ok(json!({"state": "expired"}))],
    );
    let args = ask_args(&["--wait", "--", "q"]);
    assert_eq!(rig.run(|env| ask(&mut link, &args, env)), 124);
    assert_eq!(rig.stderr().lines().last(), Some("No answer in time. (expired)"));
}

#[test]
fn without_wait_there_is_no_deadline() {
    let mut rig = Rig::new();
    rig.delegate = Some(T0 + 60_000);
    let mut script = vec![opened("q1")];
    script.extend((0..30).map(|_| Ok(json!({"state": "open"}))));
    script.push(Ok(
        json!({"state": "answered", "answer": "late", "by": "mac"}),
    ));
    let mut link = Fake::new(&rig.clock, script);
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
        0
    );
    assert!(link.of("ask.open")[0].get("deadlineMs").is_none());
    assert_eq!(link.of("ask.wait").len(), 31);
}

#[test]
fn gone_exits_1() {
    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![opened("q1"), Ok(json!({"state": "gone"}))]);
    assert_eq!(
        rig.run(|env| ask(&mut link, &ask_args(&["--", "q"]), env)),
        1
    );
    assert!(rig.stderr().contains("no longer waiting"));
}

#[test]
fn a_huge_timeout_never_overflows_the_deadline() {
    let mut rig = Rig::new();
    rig.delegate = Some(T0 + 600_000);
    let mut link = Fake::new(&rig.clock, vec![opened("q1")]);
    link.idle_waits = true;
    let args = ask_args(&["--wait", "--timeout", "9223372036854775807", "--", "q"]);
    assert_eq!(rig.run(|env| ask(&mut link, &args, env)), 124);
    assert_eq!(link.of("ask.open")[0]["deadlineMs"], json!(T0 + 540_000));
}
