use crate::contract::{self, Failure};
use crate::settings_cli::Flags;
use notchling_local_api::ops;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::Duration;

const CALL_WAIT: Duration = Duration::from_secs(15);
const POLL_MS: u64 = 25_000;

pub const USAGE: &str = "Usage: notchling session new <folder> [--model <name>] [--message <text>] [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session reply <id> <text> [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session stop <id> [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session interrupt <id> [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session remove <id> [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session resume <id> [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session answer <id> --answers <json> [--question <json>] [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session command <id> <command> [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session dialog <id> --pick <1-9>|--dismiss [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session chrome <id> on|off [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session options <id> <option> <value> [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling session reload <id|self> [--after-turn] [--json]";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    New,
    Reply,
    Stop,
    Interrupt,
    Remove,
    Resume,
    Answer,
    Command,
    Dialog,
    Chrome,
    Options,
}

impl Verb {
    fn of(word: &str) -> Option<Verb> {
        match word {
            "new" => Some(Verb::New),
            "reply" => Some(Verb::Reply),
            "stop" => Some(Verb::Stop),
            "interrupt" => Some(Verb::Interrupt),
            "remove" => Some(Verb::Remove),
            "resume" => Some(Verb::Resume),
            "answer" => Some(Verb::Answer),
            "command" => Some(Verb::Command),
            "dialog" => Some(Verb::Dialog),
            "chrome" => Some(Verb::Chrome),
            "options" => Some(Verb::Options),
            _ => None,
        }
    }

    pub fn op(self) -> &'static str {
        match self {
            Verb::New => ops::SESSION_START,
            Verb::Reply => ops::SESSION_SAY,
            Verb::Stop | Verb::Interrupt => ops::SESSION_HALT,
            Verb::Remove => ops::SESSION_DISCARD,
            Verb::Resume => ops::SESSION_RESUME,
            Verb::Answer => ops::SESSION_CHOOSE,
            Verb::Command => ops::SESSION_RUN,
            Verb::Dialog => ops::SESSION_PICK,
            Verb::Chrome => ops::SESSION_BROWSE,
            Verb::Options => ops::SESSION_TUNE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub verb: Verb,
    pub target: String,
    pub text: Option<String>,
    pub model: Option<String>,
    pub fields: Map<String, Value>,
    pub json: bool,
    pub socket: Option<PathBuf>,
    pub flags: Flags,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut words: Vec<String> = Vec::new();
    let mut tail: Option<String> = None;
    let (mut json, mut socket, mut model, mut message) = (false, None, None, None);
    let (mut answers, mut question, mut pick, mut dismiss) = (None, None, None, false);
    let mut flags = Flags::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |flag: &str| match rest.next() {
            Some(v) if !v.is_empty() => Ok(v.clone()),
            _ => Err(format!("{flag} needs a value.\n{USAGE}")),
        };
        let seconds = |text: String, flag: &str| {
            text.parse::<u64>()
                .map_err(|_| format!("{flag} takes seconds.\n{USAGE}"))
        };
        match arg.as_str() {
            "--" => {
                let text: Vec<&str> = rest.by_ref().map(String::as_str).collect();
                tail = Some(text.join(" "));
            }
            "--reason" => flags.reason = Some(value("--reason")?),
            "--key" => flags.key = Some(value("--key")?),
            "--retry-of" => flags.retry_of = Some(value("--retry-of")?),
            "--model" => model = Some(value("--model")?),
            "--message" => message = Some(value("--message")?),
            "--answers" => answers = Some(json_of(&value("--answers")?, "--answers")?),
            "--question" => question = Some(json_of(&value("--question")?, "--question")?),
            "--pick" => {
                pick = Some(
                    value("--pick")?
                        .parse::<u64>()
                        .ok()
                        .filter(|n| (1..=9).contains(n))
                        .ok_or_else(|| format!("--pick takes 1 to 9.\n{USAGE}"))?,
                )
            }
            "--dismiss" => dismiss = true,
            "--wait" => flags.wait = true,
            "--no-wait" => flags.wait = false,
            "--dry-run" => flags.dry_run = true,
            "--timeout" => {
                flags.timeout = Duration::from_secs(seconds(value("--timeout")?, "--timeout")?)
            }
            "--expires-in" => {
                flags.expires_in = Some(seconds(value("--expires-in")?, "--expires-in")?)
            }
            "--json" => json = true,
            "--socket" => socket = Some(PathBuf::from(value("--socket")?)),
            "--help" | "-h" => return Err(USAGE.to_string()),
            other if other.starts_with("--") => {
                return Err(format!("Unknown option {other}.\n{USAGE}"))
            }
            _ => words.push(arg.clone()),
        }
    }
    let verb = words
        .first()
        .and_then(|w| Verb::of(w))
        .ok_or_else(|| USAGE.to_string())?;
    let (target, text) = match (verb, words.as_slice()) {
        (Verb::Reply, [_, id]) if tail.is_some() => (id.clone(), tail.clone()),
        (Verb::New, [_, folder]) if tail.is_some() => (folder.clone(), tail.clone()),
        (Verb::Command, [_, id]) if tail.is_some() => (id.clone(), tail.clone()),
        _ if tail.is_some() => return Err(USAGE.to_string()),
        (Verb::Reply, [_, id, text]) => (id.clone(), Some(text.clone())),
        (Verb::New, [_, folder]) => (folder.clone(), message),
        (Verb::Stop | Verb::Interrupt | Verb::Remove | Verb::Resume, [_, id]) => (id.clone(), None),
        (Verb::Command, [_, id, command]) => (id.clone(), Some(command.clone())),
        (Verb::Answer | Verb::Dialog, [_, id]) => (id.clone(), None),
        (Verb::Chrome, [_, id, state]) if state == "on" || state == "off" => {
            (id.clone(), Some(state.clone()))
        }
        (Verb::Options, [_, id, option, value]) => (id.clone(), Some(format!("{option}\n{value}"))),
        _ => return Err(USAGE.to_string()),
    };
    let mut fields = Map::new();
    match verb {
        Verb::Answer => {
            let answers = answers.ok_or_else(|| format!("--answers is needed.\n{USAGE}"))?;
            fields.insert("answers".into(), answers);
            if let Some(question) = question.take() {
                let items = question.get("items").cloned().unwrap_or(question);
                fields.insert("question".into(), json!({"items": items}));
            }
        }
        Verb::Dialog => match (pick, dismiss) {
            (Some(number), false) => {
                fields.insert("number".into(), json!(number));
            }
            (None, true) => {
                fields.insert("dismiss".into(), json!(true));
            }
            _ => return Err(format!("Give --pick <1-9> or --dismiss.\n{USAGE}")),
        },
        Verb::Command => {
            fields.insert("command".into(), json!(text));
        }
        Verb::Chrome => {
            fields.insert("on".into(), json!(text.as_deref() == Some("on")));
        }
        Verb::Options => {
            let both = text.as_deref().unwrap_or_default();
            let (option, value) = both.split_once('\n').unwrap_or_default();
            fields.insert("option".into(), json!(option));
            fields.insert("value".into(), json!(value));
        }
        _ => {}
    }
    if model.is_some() && verb != Verb::New {
        return Err(format!("--model only goes with session new.\n{USAGE}"));
    }
    if target.is_empty() {
        return Err(USAGE.to_string());
    }
    Ok(Args {
        verb,
        target,
        text,
        model,
        fields,
        json,
        socket,
        flags,
    })
}

pub fn request_body(args: &Args) -> Value {
    let mut consent = Map::new();
    if let Some(reason) = &args.flags.reason {
        consent.insert("reason".into(), json!(reason));
    }
    if let Some(seconds) = args.flags.expires_in {
        consent.insert("expiresInMs".into(), json!(seconds.saturating_mul(1000)));
    }
    if let Some(key) = &args.flags.key {
        consent.insert("key".into(), json!(key));
    }
    if let Some(id) = &args.flags.retry_of {
        consent.insert("retryOf".into(), json!(id));
    }
    let mut body = match args.verb {
        Verb::New => {
            let mut body = json!({"folder": args.target});
            if let Some(model) = &args.model {
                body["model"] = json!(model);
            }
            if let Some(text) = &args.text {
                body["message"] = json!(text);
            }
            body
        }
        Verb::Reply => json!({"id": args.target, "text": args.text}),
        Verb::Stop => json!({"id": args.target, "how": "stop"}),
        Verb::Interrupt => json!({"id": args.target, "how": "interrupt"}),
        Verb::Remove | Verb::Resume => json!({"id": args.target}),
        _ => {
            let mut body = json!({"id": args.target});
            if let Some(map) = body.as_object_mut() {
                map.extend(args.fields.clone());
            }
            body
        }
    };
    body["consent"] = Value::Object(consent);
    if args.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    body
}

fn json_of(text: &str, flag: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|_| format!("{flag} takes JSON.\n{USAGE}"))
}

fn state_of(request: &Value) -> &str {
    request
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("failed")
}

pub fn done_line(verb: Verb, free: bool, session: &str) -> String {
    let tail = if free { "" } else { " (the user said yes)" };
    let what = match verb {
        Verb::New => "Started the session",
        Verb::Reply => "Sent the message",
        Verb::Stop => "Stopped the session",
        Verb::Interrupt => "Interrupted the session",
        Verb::Remove => "Removed the session",
        Verb::Resume => "Resumed the session",
        Verb::Answer => "Answered the question in the session",
        Verb::Command => "Ran the command in the session",
        Verb::Dialog => "Answered the dialog of the session",
        Verb::Chrome => "Changed Chrome for the session",
        Verb::Options => "Set the option of the session",
    };
    if session.is_empty() {
        format!("{what}{tail}.")
    } else {
        format!("{what} {session}{tail}.")
    }
}

pub fn run(raw: &[String]) -> i32 {
    let json = contract::wants_json(raw);
    let args = match parse(raw) {
        Ok(a) => a,
        Err(message) if message == USAGE && raw.iter().any(|a| a == "--help" || a == "-h") => {
            println!("{USAGE}");
            return contract::OK;
        }
        Err(message) => return Failure::usage(message).report(json),
    };
    let op = args.verb.op();
    let end = match contract::deadline(args.flags.timeout, USAGE) {
        Ok(end) => end,
        Err(failure) => return failure.report(json),
    };
    let mut client = match contract::connect(args.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(CALL_WAIT);
    let first = match contract::call(&mut client, op, request_body(&args)) {
        Ok(reply) => reply,
        Err(failure) => return failure.report(json),
    };
    if first.get("free").and_then(Value::as_bool) == Some(true) {
        if json {
            println!("{}", contract::pretty(&first));
        } else {
            let id = first.get("session").and_then(Value::as_str).unwrap_or("");
            println!("{}", done_line(args.verb, true, id));
        }
        return contract::OK;
    }
    if args.flags.dry_run {
        println!("{}", contract::pretty(&first));
        return contract::OK;
    }
    let request = first.get("request").cloned().unwrap_or(Value::Null);
    let id = request
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let ask = |left: Duration| {
        let slice = contract::wait_slice(left, POLL_MS);
        (
            json!({"id": id, "waitMs": slice}),
            CALL_WAIT + Duration::from_millis(slice),
        )
    };
    let request = match contract::wait_while_pending(request, args.flags.wait, end, || {
        contract::call_through_restart(
            &mut client,
            args.socket.as_deref(),
            ops::CONSENT_GET,
            &ask,
            end,
        )
    }) {
        Ok(request) => request,
        Err(failure) => return failure.report(json),
    };
    if state_of(&request) != "applied" {
        return crate::consent_cli::failure_of(&request, op).report(json);
    }
    if json {
        println!("{}", contract::pretty(&json!({"request": request})));
    } else {
        let session = request
            .pointer("/result/session")
            .or_else(|| request.pointer("/result/jobId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        println!("{}", done_line(args.verb, false, session));
    }
    contract::OK
}

#[cfg(test)]
#[path = "session_cli_tests.rs"]
mod tests;
