use crate::contract::{self, Failure};
use notchling_local_api::{ops, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const USAGE: &str = "Usage: notchling consent <id> [--wait] [--timeout <seconds>] [--json]\n       notchling consents [--json]\n       notchling consent cancel <id> [--json]";

const POLL_MS: u64 = 25_000;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(900);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sub {
    Get(String),
    List,
    Cancel(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub sub: Sub,
    pub json: bool,
    pub wait: bool,
    pub timeout: Duration,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String], list: bool) -> Result<Args, String> {
    let mut words: Vec<String> = Vec::new();
    let (mut json, mut wait, mut socket) = (false, false, None);
    let mut timeout = DEFAULT_TIMEOUT;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--wait" => wait = true,
            "--timeout" => {
                let seconds: u64 = rest
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| format!("--timeout takes seconds.\n{USAGE}"))?;
                timeout = Duration::from_secs(seconds);
            }
            "--socket" => match rest.next() {
                Some(path) if !path.is_empty() => socket = Some(PathBuf::from(path)),
                _ => return Err(format!("--socket needs a value.\n{USAGE}")),
            },
            "--help" | "-h" => return Err(USAGE.to_string()),
            other if other.starts_with("--") => {
                return Err(format!("unknown flag {other}.\n{USAGE}"))
            }
            _ => words.push(arg.clone()),
        }
    }
    let sub = match (list, words.as_slice()) {
        (true, []) => Sub::List,
        (false, [id]) => Sub::Get(id.clone()),
        (false, [cancel, id]) if cancel == "cancel" => Sub::Cancel(id.clone()),
        _ => return Err(USAGE.to_string()),
    };
    Ok(Args {
        sub,
        json,
        wait,
        timeout,
        socket,
    })
}

pub fn exit_of(state: &str) -> i32 {
    match state {
        "applied" => contract::OK,
        "waiting" | "opened" | "applying" => contract::PENDING,
        "denied" | "cancelled" => contract::DENIED,
        "expired" => contract::EXPIRED,
        _ => contract::FAILED,
    }
}

fn state_of(request: &Value) -> &str {
    request
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("failed")
}

pub fn line(request: &Value) -> String {
    let id = request.get("id").and_then(Value::as_str).unwrap_or("?");
    let target = request.get("target").and_then(Value::as_str).unwrap_or("");
    let kind = request.get("kind").and_then(Value::as_str).unwrap_or("");
    format!("{id}  {}  {kind} {target}", state_of(request))
}

pub fn failure_of(request: &Value, op: &str) -> Failure {
    let state = state_of(request);
    let code = exit_of(state);
    let (reason, message) = match state {
        "waiting" | "opened" | "applying" => {
            ("needs_consent", "The request still waits for the user.")
        }
        "denied" => ("denied", "The user denied this request."),
        "cancelled" => ("cancelled", "The request was withdrawn."),
        "expired" => ("expired", "The user didn't answer in time."),
        "withdrawn" => (
            "withdrawn",
            "The request was withdrawn before it was applied.",
        ),
        _ => ("failed", "The request didn't go through."),
    };
    let mut failure = Failure::new(code, reason, message)
        .with_op(op)
        .with_human(format!("{} ({message})", line(request)));
    failure.request = Some(request.clone());
    failure
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    );
}

pub fn run(args: &[String], list: bool) -> i32 {
    let json = contract::wants_json(args);
    let parsed = match parse(args, list) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let mut client = match contract::connect(parsed.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(CALL_TIMEOUT + Duration::from_millis(POLL_MS));
    match &parsed.sub {
        Sub::List => {
            let reply = match contract::call(&mut client, ops::CONSENT_MINE, json!({})) {
                Ok(r) => r,
                Err(f) => return f.report(json),
            };
            if json {
                print_json(&reply);
            } else {
                let rows = reply["requests"].as_array().cloned().unwrap_or_default();
                if rows.is_empty() {
                    println!("No requests from this session.");
                }
                for row in &rows {
                    println!("{}", line(row));
                }
            }
            contract::OK
        }
        Sub::Cancel(id) => {
            let reply = match contract::call(&mut client, ops::CONSENT_CANCEL, json!({"id": id})) {
                Ok(r) => r,
                Err(f) => return f.report(json),
            };
            if reply["cancelled"] != json!(true) {
                let mut failure = Failure::failed(format!(
                    "The request is already {}.",
                    reply["state"].as_str().unwrap_or("closed")
                ))
                .with_op(ops::CONSENT_CANCEL);
                failure.request =
                    contract::call(&mut client, ops::CONSENT_GET, json!({"id": id})).ok();
                return failure.report(json);
            }
            if json {
                print_json(&reply);
            } else {
                println!("Withdrew {id}.");
            }
            contract::OK
        }
        Sub::Get(id) => {
            let end = Instant::now() + parsed.timeout;
            let mut request;
            loop {
                let wait_ms = if parsed.wait {
                    POLL_MS.min(
                        u64::try_from(end.saturating_duration_since(Instant::now()).as_millis())
                            .unwrap_or(POLL_MS),
                    )
                } else {
                    0
                };
                let until = if parsed.wait { end } else { Instant::now() };
                request = match contract::call_through_restart(
                    &mut client,
                    (
                        parsed.socket.as_deref(),
                        CALL_TIMEOUT + Duration::from_millis(POLL_MS),
                    ),
                    ops::CONSENT_GET,
                    &json!({"id": id, "waitMs": wait_ms}),
                    until,
                ) {
                    Ok(r) => r,
                    Err(f) => return f.report(json),
                };
                let pending = exit_of(state_of(&request)) == contract::PENDING;
                if !pending || !parsed.wait || Instant::now() >= end {
                    break;
                }
            }
            let code = exit_of(state_of(&request));
            if code != contract::OK {
                return failure_of(&request, ops::CONSENT_GET).report(json);
            }
            if json {
                print_json(&request);
            } else {
                println!("{}", line(&request));
            }
            contract::OK
        }
    }
}

#[cfg(test)]
#[path = "consent_cli_tests.rs"]
mod tests;
