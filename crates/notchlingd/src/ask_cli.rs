use notchling_engine::ask::{DEADLINE_MARGIN_MS, WAIT_DEFAULT_S, WAIT_MS_MAX};
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::io::Write;
use std::time::Duration;

pub(crate) use crate::ask_cli_args::{
    parse_answer, parse_ask, parse_asks, AnswerArgs, AskArgs, AsksArgs,
};
pub use crate::ask_cli_run::{run_answer, run_ask, run_asks};

const NOT_RUNNING: &str = "Notchling isn't running. Ask the user to open it, then try again.";
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

fn refusal_exit(e: &ApiError) -> i32 {
    match e.code {
        ErrorCode::Forbidden | ErrorCode::BadRequest | ErrorCode::Failed => 3,
        _ => 1,
    }
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

fn open_question(link: &mut dyn AskLink, env: &mut Env, args: &Value) -> Result<String, i32> {
    let started = (env.now)();
    let mut backoff = Backoff::new();
    loop {
        match link.call(ops::ASK_OPEN, args, OPEN_TIMEOUT) {
            Ok(reply) => {
                return match reply.get("id").and_then(Value::as_str) {
                    Some(id) => Ok(id.to_string()),
                    None => {
                        say(env.err, "Notchling didn't answer.");
                        Err(1)
                    }
                }
            }
            Err(LinkError::Api(e)) => {
                say(env.err, &e.message);
                return Err(refusal_exit(&e));
            }
            Err(LinkError::Fatal(message)) => {
                say(env.err, &message);
                return Err(1);
            }
            Err(LinkError::Down) => {
                let left = OPEN_RETRY_MS - ((env.now)() - started);
                if left <= 0 {
                    say(env.err, NOT_RUNNING);
                    return Err(3);
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
    if args.json {
        say(env.out, &record(id, state, reply).to_string());
    }
    match state {
        "answered" => {
            if !args.json {
                let text = reply.get("answer").and_then(Value::as_str).unwrap_or("");
                say(env.out, &format!("Answer: {text}"));
                if let Some(line) = from_line(reply.get("by").and_then(Value::as_str)) {
                    say(env.out, line);
                }
            }
            0
        }
        "dismissed" => {
            say(env.err, DISMISSED);
            2
        }
        "expired" => {
            say(env.err, IN_TIME);
            124
        }
        _ => {
            say(env.err, NO_LONGER);
            1
        }
    }
}

pub(crate) fn ask(link: &mut dyn AskLink, args: &AskArgs, env: &mut Env) -> i32 {
    let deadline = deadline_for(args, env);
    let opening = open_args(args, env, deadline);
    let id = match open_question(link, env, &opening) {
        Ok(id) => id,
        Err(code) => return code,
    };
    say(env.err, &format!("Asked ({id}). Waiting for the answer."));
    let mut backoff = Backoff::new();
    loop {
        let wait_ms = match remaining(env, deadline) {
            Some(left) if left <= 0 => {
                say(env.err, IN_TIME);
                return 124;
            }
            Some(left) => left.min(WAIT_MS_MAX),
            None => WAIT_MS_MAX,
        };
        let call = json!({"id": id, "waitMs": wait_ms});
        match persistent(link, env, &mut backoff, ops::ASK_WAIT, &call, deadline) {
            Ok(reply) => match reply.get("state").and_then(Value::as_str) {
                Some("open") | None => {}
                Some(state) => return finish(env, args, &id, state, &reply),
            },
            Err(Stop::Late) => {
                say(env.err, IN_TIME);
                return 124;
            }
            Err(Stop::Api(e)) => {
                say(env.err, &e.message);
                return 1;
            }
            Err(Stop::Fatal(message)) => {
                say(env.err, &message);
                return 1;
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
                    say(env.err, NOT_RUNNING);
                    return 3;
                }
                Err(LinkError::Api(e)) => Err(Stop::Api(e)),
                Err(LinkError::Fatal(message)) => Err(Stop::Fatal(message)),
            }
        };
        match reply {
            Ok(Value::Array(rows)) if rows.is_empty() && args.wait => {}
            Ok(Value::Array(rows)) => break rows,
            Ok(_) => {
                say(env.err, "Notchling didn't answer.");
                return 1;
            }
            Err(Stop::Api(e)) => {
                say(env.err, &e.message);
                return refusal_exit(&e);
            }
            Err(Stop::Fatal(message)) => {
                say(env.err, &message);
                return 1;
            }
            Err(Stop::Late) => return 1,
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
            say(env.out, "Answered.");
            0
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
            say(env.out, &line);
            2
        }
        Err(LinkError::Down) => {
            say(env.err, NOT_RUNNING);
            3
        }
        Err(LinkError::Api(e)) => {
            say(env.err, &e.message);
            refusal_exit(&e)
        }
        Err(LinkError::Fatal(message)) => {
            say(env.err, &message);
            1
        }
    }
}

#[cfg(test)]
#[path = "ask_cli_list_tests.rs"]
mod list_tests;
#[cfg(test)]
#[path = "ask_cli_tests.rs"]
mod tests;
