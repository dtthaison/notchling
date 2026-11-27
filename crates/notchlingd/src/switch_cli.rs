use crate::contract::{self, Failure};
use crate::settings_cli::Flags;
use crate::switch_asks::NAMES;
use notchling_local_api::ops;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::Duration;

const CALL_WAIT: Duration = Duration::from_secs(15);
const POLL_MS: u64 = 25_000;

pub const USAGE: &str = "Usage: notchling switch on <acting|bridge|accounts|vault|install|cluster|machines> [--reason <why>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]\n       notchling switch off <acting|bridge|accounts|vault|install|cluster|machines> [--json]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub on: bool,
    pub name: String,
    pub json: bool,
    pub socket: Option<PathBuf>,
    pub flags: Flags,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut words: Vec<String> = Vec::new();
    let mut json = false;
    let mut socket = None;
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
            "--reason" => flags.reason = Some(value("--reason")?),
            "--key" => flags.key = Some(value("--key")?),
            "--retry-of" => flags.retry_of = Some(value("--retry-of")?),
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
    let (on, name) = match words.as_slice() {
        [verb, name] if verb == "on" => (true, name.clone()),
        [verb, name] if verb == "off" => (false, name.clone()),
        [verb] if verb == "on" || verb == "off" => {
            return Err(format!("switch {verb} needs a switch.\n{USAGE}"))
        }
        _ => return Err(USAGE.to_string()),
    };
    if !NAMES.contains(&name.as_str()) {
        return Err(format!("No switch named {name}.\n{USAGE}"));
    }
    Ok(Args {
        on,
        name,
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
    let mut body = json!({"switch": args.name, "on": true, "consent": consent});
    if args.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    body
}

fn state_of(request: &Value) -> &str {
    request
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("failed")
}

pub fn off_line(reply: &Value) -> String {
    let name = reply.get("switch").and_then(Value::as_str).unwrap_or("");
    if reply.get("changed").and_then(Value::as_bool) == Some(true) {
        format!("{name} is off.")
    } else {
        format!("{name} was already off.")
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
    let end = match contract::deadline(args.flags.timeout, USAGE) {
        Ok(end) => end,
        Err(failure) => return failure.report(json),
    };
    let mut client = match contract::connect(args.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(CALL_WAIT);
    if !args.on {
        return match contract::call(&mut client, ops::SWITCH_OFF, json!({"switch": args.name})) {
            Ok(reply) if json => {
                println!("{}", contract::pretty(&reply));
                contract::OK
            }
            Ok(reply) => {
                println!("{}", off_line(&reply));
                contract::OK
            }
            Err(failure) => failure.report(json),
        };
    }
    let first = match contract::call(&mut client, ops::SWITCH_REQUEST, request_body(&args)) {
        Ok(reply) => reply,
        Err(failure) => return failure.report(json),
    };
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
        return crate::consent_cli::failure_of(&request, ops::SWITCH_REQUEST).report(json);
    }
    if json {
        println!("{}", contract::pretty(&json!({"request": request})));
    } else {
        println!("{} is on for this run.", args.name);
    }
    contract::OK
}

#[cfg(test)]
#[path = "switch_cli_tests.rs"]
mod tests;
