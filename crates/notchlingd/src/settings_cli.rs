use crate::contract::{self, Failure};
use crate::settings_asks::ASK_WAIT;
use notchling_local_api::{ops, ClientError, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const USAGE: &str = "Usage: notchling settings list [--json]\n       notchling settings get <key> [--json]\n       notchling settings set <key> <value> [--json]";

pub const WAITING: &str =
    "Waiting for the user to allow this in Notchling's Inbox (on the Mac or a paired phone)…";

const POLL: u64 = 25;
const POLL_TIMEOUT: Duration = Duration::from_secs(POLL + 15);

pub const DONE: i32 = contract::OK;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sub {
    List,
    Get(String),
    Set(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub sub: Sub,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut words: Vec<String> = Vec::new();
    let mut json = false;
    let mut socket = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--socket" => match rest.next() {
                Some(path) if !path.is_empty() => socket = Some(PathBuf::from(path)),
                _ => return Err(format!("--socket needs a value.\n{USAGE}")),
            },
            "--help" | "-h" => return Err(USAGE.to_string()),
            _ => words.push(arg.clone()),
        }
    }
    let sub = match words.as_slice() {
        [list] if list == "list" => Sub::List,
        [get, key] if get == "get" => Sub::Get(key.clone()),
        [set, key, value] if set == "set" => Sub::Set(key.clone(), value.clone()),
        [set, _] if set == "set" => {
            return Err(format!("settings set needs a key and a value.\n{USAGE}"))
        }
        [get] if get == "get" => return Err(format!("settings get needs a key.\n{USAGE}")),
        _ => return Err(USAGE.to_string()),
    };
    Ok(Args { sub, json, socket })
}

pub fn value_text(value: &Value) -> String {
    match value {
        Value::Null => "unset".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn str_of<'a>(entry: &'a Value, field: &str) -> &'a str {
    entry.get(field).and_then(Value::as_str).unwrap_or_default()
}

pub fn takes(entry: &Value) -> String {
    if let Some(allowed) = entry.get("allowed").and_then(Value::as_array) {
        let names: Vec<String> = allowed.iter().map(value_text).collect();
        return format!("one of {}", names.join(", "));
    }
    if let Some([low, high]) = entry
        .get("range")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    {
        return format!("a whole number from {low} to {high}");
    }
    match str_of(entry, "kind") {
        "bool" => "true or false",
        "int" => "a whole number",
        "number" => "a number",
        "string" => "text",
        "list" => "a list of texts",
        "object" => "an object",
        _ => "a JSON value",
    }
    .to_string()
}

fn shown_value(entry: &Value) -> Option<String> {
    let value = entry.get("value")?;
    if entry.get("switch").and_then(Value::as_bool) == Some(true) {
        return Some(
            match value.as_bool() {
                Some(true) => "on",
                Some(false) => "off",
                None => "unknown",
            }
            .to_string(),
        );
    }
    Some(value_text(value))
}

fn line(entry: &Value) -> String {
    let key = str_of(entry, "key");
    let title = str_of(entry, "title");
    match (shown_value(entry), str_of(entry, "class")) {
        (Some(value), "never") => format!("  {key} = {value}   {title}"),
        (Some(value), _) => format!("  {key} = {value}   {title} ({})", takes(entry)),
        (None, _) => format!("  {key}   {title}"),
    }
}

pub fn list_lines(reply: &Value) -> Vec<String> {
    let all = reply
        .get("settings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (class, heading) in [
        ("free", "You may change these:"),
        ("ask", "These ask the user first, in Notchling's Inbox:"),
        ("never", "Only the user changes these, in Notchling:"),
    ] {
        let rows: Vec<String> = all
            .iter()
            .filter(|e| str_of(e, "class") == class)
            .map(line)
            .collect();
        if rows.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(heading.to_string());
        out.extend(rows);
    }
    out
}

pub fn entry_lines(entry: &Value) -> Vec<String> {
    let key = str_of(entry, "key");
    let mut out = vec![match shown_value(entry) {
        Some(value) => format!("{key} = {value}"),
        None => key.to_string(),
    }];
    out.push(format!(
        "{}: {}.",
        str_of(entry, "title"),
        str_of(entry, "description").trim_end_matches('.')
    ));
    let default = entry
        .get("default")
        .filter(|d| !d.is_null())
        .map(|d| format!(" Default {}.", value_text(d)))
        .unwrap_or_default();
    out.push(format!("Takes {}.{default}", takes(entry)));
    out.push(match str_of(entry, "class") {
        "free" => "Agents may change it.".to_string(),
        "ask" => "Agents may ask; the user allows or denies it in Notchling's Inbox.".to_string(),
        _ => format!("Agents can't change it: {}.", str_of(entry, "why")),
    });
    for pending in entry
        .get("pending")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.push(format!(
            "Waiting for the user's approval: {}.",
            value_text(pending.get("value").unwrap_or(&Value::Null))
        ));
    }
    out
}

pub fn outcome(reply: &Value) -> (i32, String) {
    let key = str_of(reply, "key");
    let value = value_text(reply.get("value").unwrap_or(&Value::Null));
    let message = str_of(reply, "message").to_string();
    match str_of(reply, "state") {
        "applied" if reply.get("changed") == Some(&Value::Bool(false)) => {
            (DONE, format!("{key} is already {value}."))
        }
        "applied" => (DONE, format!("Set {key} to {value}.")),
        "denied" => (
            contract::DENIED,
            format!("The user denied setting {key} to {value}."),
        ),
        "expired" => (
            contract::EXPIRED,
            format!("The user didn't answer in time, so {key} is unchanged. Ask them, then try again."),
        ),
        "withdrawn" | "failed" => (contract::FAILED, message),
        _ => (
            contract::PENDING,
            format!("Still waiting for the user's approval in Notchling's Inbox. {key} changes to {value} once they allow it."),
        ),
    }
}

fn failure_of(reply: &Value, code: i32, text: String) -> Failure {
    let (reason, message) = match code {
        contract::FAILED if str_of(reply, "state") == "withdrawn" => {
            ("withdrawn", "The request was withdrawn.")
        }
        contract::DENIED => ("denied", "The user denied this change."),
        contract::EXPIRED => ("expired", "The user didn't answer in time."),
        contract::PENDING => ("needs_consent", "The request still waits for the user."),
        _ => ("failed", "The change didn't happen."),
    };
    let mut failure = Failure::new(code, reason, message)
        .with_op(ops::SETTINGS_SET)
        .with_human(text);
    failure.request = Some(reply.clone());
    failure
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    );
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

type Call<'a> = dyn FnMut(&str, Value, Duration) -> Result<Value, ClientError> + 'a;

fn set(
    call: &mut Call<'_>,
    key: &str,
    text: &str,
    quiet: bool,
) -> Result<Value, (&'static str, ClientError)> {
    let first = call(
        ops::SETTINGS_SET,
        json!({"key": key, "value": text}),
        CALL_TIMEOUT,
    )
    .map_err(|e| (ops::SETTINGS_SET, e))?;
    if str_of(&first, "state") != "waiting" {
        return Ok(first);
    }
    if !quiet {
        eprintln!("{WAITING}");
    }
    let id = str_of(&first, "id").to_string();
    let end = Instant::now() + ASK_WAIT + Duration::from_secs(15);
    let mut last = first;
    while Instant::now() < end {
        last = call(
            ops::SETTINGS_ASKED,
            json!({"id": id, "wait": POLL}),
            POLL_TIMEOUT,
        )
        .map_err(|e| (ops::SETTINGS_ASKED, e))?;
        if !matches!(str_of(&last, "state"), "waiting" | "applying") {
            break;
        }
    }
    Ok(last)
}

pub fn run(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let mut client = match contract::connect(parsed.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    let mut call = |op: &str, args: Value, timeout: Duration| {
        client.set_timeout(timeout);
        client.call(op, args)
    };
    let plain = |reply: Result<Value, ClientError>, op: &'static str| reply.map_err(|e| (op, e));
    let reply = match &parsed.sub {
        Sub::List => plain(
            call(ops::SETTINGS_LIST, json!({}), CALL_TIMEOUT),
            ops::SETTINGS_LIST,
        ),
        Sub::Get(key) => plain(
            call(ops::SETTINGS_GET, json!({"key": key}), CALL_TIMEOUT),
            ops::SETTINGS_GET,
        ),
        Sub::Set(key, value) => set(&mut call, key, value, json),
    };
    let reply = match reply {
        Ok(reply) => reply,
        Err((op, e)) => return Failure::from_client(Some(op), e).report(json),
    };
    if let Sub::Set(..) = parsed.sub {
        let (code, text) = outcome(&reply);
        if code != DONE {
            return failure_of(&reply, code, text).report(json);
        }
        if !json {
            println!("{text}");
        } else {
            print_json(&reply);
        }
        return DONE;
    }
    if json {
        print_json(&reply);
        return DONE;
    }
    match parsed.sub {
        Sub::List => print_lines(&list_lines(&reply)),
        _ => print_lines(&entry_lines(&reply)),
    }
    DONE
}

#[cfg(test)]
#[path = "settings_cli_tests.rs"]
mod tests;
