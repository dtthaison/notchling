use crate::contract::{self, Failure};
use notchling_engine::ask::{DEADLINE_MARGIN_MS, WAIT_DEFAULT_S, WAIT_MS_MAX};
use notchling_local_api::{ops, ApiError};
use serde_json::{json, Value};
use std::io::Write;
use std::time::Duration;

pub(crate) use crate::ask_cli_args::{
    parse_answer, parse_ask, parse_asks, AnswerArgs, AskArgs, AsksArgs,
};
pub use crate::ask_cli_run::{run_answer, run_ask, run_asks};

const DISMISSED: &str = "No answer: the question was dismissed.";
const IN_TIME: &str = "No answer in time.";
const NO_LONGER: &str = "The question is no longer waiting.";
const NONE_WAITING: &str = "No questions waiting.";
const OPEN_RETRY_MS: i64 = 60_000;
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const CALL_TIMEOUT: Duration = Duration::from_secs(15);
const STEPS_MS: [u64; 4] = [1_000, 2_000, 5_000, 10_000];

pub(crate) enum LinkError {
    Down,
    Api(ApiError),
    Fatal(String),
}

pub(crate) trait AskLink {
    fn call(&mut self, op: &str, args: &Value, timeout: Duration) -> Result<Value, LinkError>;
    fn reconnect(&mut self) -> Result<(), LinkError>;
}

pub(crate) struct Backoff {
    step: usize,
}

impl Backoff {
    pub(crate) fn new() -> Backoff {
        Backoff { step: 0 }
    }

    pub(crate) fn next_ms(&mut self) -> u64 {
        let ms = STEPS_MS[self.step.min(STEPS_MS.len() - 1)];
        self.step = self.step.saturating_add(1);
        ms
    }

    fn reset(&mut self) {
        self.step = 0;
    }
}

pub(crate) struct Env<'a> {
    pub now: &'a dyn Fn() -> i64,
    pub sleep: &'a dyn Fn(u64),
    pub delegate_deadline: Option<i64>,
    pub key: String,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

enum Stop {
    Late,
    Api(ApiError),
    Fatal(String),
}

fn say(out: &mut dyn Write, line: &str) {
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn remaining(env: &Env, deadline: Option<i64>) -> Option<i64> {
    deadline.map(|d| d - (env.now)())
}

fn persistent(
    link: &mut dyn AskLink,
    env: &mut Env,
    backoff: &mut Backoff,
    op: &str,
    args: &Value,
    deadline: Option<i64>,
) -> Result<Value, Stop> {
    loop {
        if remaining(env, deadline).is_some_and(|left| left <= 0) {
            return Err(Stop::Late);
        }
        match link.call(op, args, WAIT_TIMEOUT) {
            Ok(reply) => {
                backoff.reset();
                return Ok(reply);
            }
            Err(LinkError::Api(e)) => return Err(Stop::Api(e)),
            Err(LinkError::Fatal(message)) => return Err(Stop::Fatal(message)),
            Err(LinkError::Down) => loop {
                let mut ms = backoff.next_ms();
                if let Some(left) = remaining(env, deadline) {
                    if left <= 0 {
                        return Err(Stop::Late);
                    }
                    ms = ms.min(left as u64);
                }
                (env.sleep)(ms);
                match link.reconnect() {
                    Ok(()) => break,
                    Err(LinkError::Fatal(message)) => return Err(Stop::Fatal(message)),
                    Err(_) => {}
                }
            },
        }
    }
}

fn stop(env: &mut Env, json: bool, failure: Failure) -> i32 {
    failure.emit(json, env.out, env.err)
}

fn expired() -> Failure {
    Failure::new(contract::EXPIRED, "expired", IN_TIME)
}

fn deadline_for(args: &AskArgs, env: &Env) -> Option<i64> {
    if !args.wait {
        return None;
    }
    let seconds = args.timeout_s.unwrap_or(WAIT_DEFAULT_S);
    let span = i64::try_from(seconds)
        .unwrap_or(i64::MAX)
        .saturating_mul(1000);
    let own = (env.now)().saturating_add(span);
    Some(match env.delegate_deadline {
        Some(delegate) => own.min(delegate.saturating_sub(DEADLINE_MARGIN_MS)),
        None => own,
    })
}

fn open_args(args: &AskArgs, env: &Env, deadline: Option<i64>) -> Value {
    let mut out = json!({"question": args.question, "key": env.key});
    if !args.choices.is_empty() {
        out["choices"] = json!(args.choices);
    }
    if args.main_may_answer {
        out["mainMayAnswer"] = json!(true);
    }
    if args.wait {
        out["wait"] = json!(true);
    }
    if let Some(deadline) = deadline {
        out["deadlineMs"] = json!(deadline);
    }
    out
}

fn open_question(
    link: &mut dyn AskLink,
    env: &mut Env,
    args: &Value,
    json: bool,
) -> Result<String, i32> {
    let started = (env.now)();
    let mut backoff = Backoff::new();
    loop {
        match link.call(ops::ASK_OPEN, args, OPEN_TIMEOUT) {
            Ok(reply) => {
                return match reply.get("id").and_then(Value::as_str) {
                    Some(id) => Ok(id.to_string()),
                    None => Err(stop(
                        env,
                        json,
                        Failure::failed("Notchling didn't answer.").with_op(ops::ASK_OPEN),
                    )),
                }
            }
            Err(LinkError::Api(e)) => {
                let failure = Failure::from_api(Some(ops::ASK_OPEN), e);
                return Err(stop(env, json, failure));
            }
            Err(LinkError::Fatal(message)) => {
                return Err(stop(env, json, Failure::failed(message)));
            }
            Err(LinkError::Down) => {
                let left = OPEN_RETRY_MS - ((env.now)() - started);
                if left <= 0 {
                    return Err(stop(
                        env,
                        json,
                        Failure::not_running().with_op(ops::ASK_OPEN),
                    ));
                }
                (env.sleep)(backoff.next_ms().min(left as u64));
            }
        }
    }
}

fn from_line(by: Option<&str>) -> Option<&'static str> {
    match by? {
        "mac" => Some("From: you (Mac)"),
        "phone" => Some("From: you (phone)"),
        "main" => Some("From: the main session"),
        _ => None,
    }
}

fn record(id: &str, state: &str, reply: &Value) -> Value {
    json!({
        "id": id,
        "state": state,
        "answer": reply.get("answer").cloned().unwrap_or(Value::Null),
        "choice": reply.get("choice").cloned().unwrap_or(Value::Null),
        "by": reply.get("by").cloned().unwrap_or(Value::Null),
    })
}

fn finish(env: &mut Env, args: &AskArgs, id: &str, state: &str, reply: &Value) -> i32 {
    let recorded = record(id, state, reply);
    let failure = match state {
        "answered" => {
            if args.json {
                say(env.out, &recorded.to_string());
            } else {
                let text = reply.get("answer").and_then(Value::as_str).unwrap_or("");
                say(env.out, &format!("Answer: {text}"));
                if let Some(line) = from_line(reply.get("by").and_then(Value::as_str)) {
                    say(env.out, line);
                }
            }
            return contract::OK;
        }
        "dismissed" => Failure::new(contract::DENIED, "denied", DISMISSED),
        "expired" => expired(),
        _ => Failure::new(contract::FAILED, "withdrawn", NO_LONGER),
    };
    let mut failure = failure.with_op(ops::ASK_WAIT);
    failure.request = Some(recorded);
    stop(env, args.json, failure)
}

pub(crate) fn ask(link: &mut dyn AskLink, args: &AskArgs, env: &mut Env) -> i32 {
    let deadline = deadline_for(args, env);
    let opening = open_args(args, env, deadline);
    let id = match open_question(link, env, &opening, args.json) {
        Ok(id) => id,
        Err(code) => return code,
    };
    if !args.json {
        say(env.err, &format!("Asked ({id}). Waiting for the answer."));
    }
    let mut backoff = Backoff::new();
    loop {
        let wait_ms = match remaining(env, deadline) {
            Some(left) if left <= 0 => return stop(env, args.json, expired()),
            Some(left) => left.min(WAIT_MS_MAX),
            None => WAIT_MS_MAX,
        };
        let call = json!({"id": id, "waitMs": wait_ms});
        match persistent(link, env, &mut backoff, ops::ASK_WAIT, &call, deadline) {
            Ok(reply) => match reply.get("state").and_then(Value::as_str) {
                Some("open") | None => {}
                Some(state) => return finish(env, args, &id, state, &reply),
            },
            Err(Stop::Late) => return stop(env, args.json, expired()),
            Err(Stop::Api(e)) if e.code == notchling_local_api::ErrorCode::BadRequest => {
                let failure = Failure::new(contract::FAILED, "withdrawn", NO_LONGER);
                return stop(env, args.json, failure.with_op(ops::ASK_WAIT));
            }
            Err(Stop::Api(e)) => {
                let failure = Failure::from_api(Some(ops::ASK_WAIT), e);
                return stop(env, args.json, failure);
            }
            Err(Stop::Fatal(message)) => {
                return stop(env, args.json, Failure::failed(message));
            }
        }
    }
}

fn waited(ms: i64) -> String {
    let seconds = ms.max(0) / 1000;
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

fn show_row(env: &mut Env, row: &Value) {
    let text = |key: &str| row.get(key).and_then(Value::as_str);
    let mut head = format!("{}  ", text("id").unwrap_or("?"));
    if let Some(label) = text("label") {
        head.push_str(&format!("«{label}» "));
    }
    head.push_str(&format!("({})", text("asker").unwrap_or("unknown")));
    if let Some(branch) = text("branch") {
        head.push_str(&format!(" · {branch}"));
    }
    let since = row.get("createdMs").and_then(Value::as_i64);
    if let Some(since) = since {
        head.push_str(&format!(" · {}", waited((env.now)() - since)));
    }
    say(env.out, &head);
    say(env.out, text("question").unwrap_or(""));
    let choices = row.get("choices").and_then(Value::as_array);
    for (n, choice) in choices.into_iter().flatten().enumerate() {
        say(
            env.out,
            &format!("  {}. {}", n + 1, choice.as_str().unwrap_or("")),
        );
    }
}

pub(crate) fn asks(link: &mut dyn AskLink, args: &AsksArgs, env: &mut Env) -> i32 {
    let call = if args.wait {
        json!({"waitMs": WAIT_MS_MAX})
    } else {
        json!({})
    };
    let mut backoff = Backoff::new();
    let rows = loop {
        let reply = if args.wait {
            persistent(link, env, &mut backoff, ops::ASK_MINE, &call, None)
        } else {
            match link.call(ops::ASK_MINE, &call, CALL_TIMEOUT) {
                Ok(reply) => Ok(reply),
                Err(LinkError::Down) => {
                    let failure = Failure::not_running().with_op(ops::ASK_MINE);
                    return stop(env, args.json, failure);
                }
                Err(LinkError::Api(e)) => Err(Stop::Api(e)),
                Err(LinkError::Fatal(message)) => Err(Stop::Fatal(message)),
            }
        };
        match reply {
            Ok(Value::Array(rows)) if rows.is_empty() && args.wait => {}
            Ok(Value::Array(rows)) => break rows,
            Ok(_) => {
                let failure = Failure::failed("Notchling didn't answer.").with_op(ops::ASK_MINE);
                return stop(env, args.json, failure);
            }
            Err(Stop::Api(e)) => {
                let failure = Failure::from_api(Some(ops::ASK_MINE), e);
                return stop(env, args.json, failure);
            }
            Err(Stop::Fatal(message)) => {
                return stop(env, args.json, Failure::failed(message));
            }
            Err(Stop::Late) => return contract::FAILED,
        }
    };
    if args.json {
        say(env.out, &Value::Array(rows).to_string());
    } else if rows.is_empty() {
        say(env.out, NONE_WAITING);
    } else {
        for row in &rows {
            show_row(env, row);
        }
    }
    0
}

fn answerer(by: &str) -> String {
    match by {
        "mac" => "Mac".to_string(),
        "phone" => "Phone".to_string(),
        "main" => "Main session".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn answer(link: &mut dyn AskLink, args: &AnswerArgs, env: &mut Env) -> i32 {
    let mut call = json!({"id": args.id});
    if let Some(choice) = args.choice {
        call["choice"] = json!(choice);
    }
    if let Some(text) = &args.text {
        call["text"] = json!(text);
    }
    match link.call(ops::ASK_ANSWER, &call, CALL_TIMEOUT) {
        Ok(reply) if reply.get("answered") == Some(&json!(true)) => {
            if args.json {
                say(env.out, &json!({"answered": true}).to_string());
            } else {
                say(env.out, "Answered.");
            }
            contract::OK
        }
        Ok(reply) => {
            let line = match reply.get("by").and_then(Value::as_str) {
                Some(by) => format!("Already answered · {}.", answerer(by)),
                None => {
                    let state = reply
                        .get("state")
                        .and_then(Value::as_str)
                        .unwrap_or("closed");
                    format!("Already {state}.")
                }
            };
            let failure = Failure::new(contract::FAILED, "already_answered", NO_LONGER)
                .with_op(ops::ASK_ANSWER)
                .with_human(line);
            stop(env, args.json, failure)
        }
        Err(LinkError::Down) => stop(
            env,
            args.json,
            Failure::not_running().with_op(ops::ASK_ANSWER),
        ),
        Err(LinkError::Api(e)) => {
            let failure = Failure::from_api(Some(ops::ASK_ANSWER), e);
            stop(env, args.json, failure)
        }
        Err(LinkError::Fatal(message)) => stop(env, args.json, Failure::failed(message)),
    }
}

#[cfg(test)]
#[path = "ask_cli_list_tests.rs"]
mod list_tests;
#[cfg(test)]
#[path = "ask_cli_tests.rs"]
mod tests;
