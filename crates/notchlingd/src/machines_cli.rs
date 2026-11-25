use crate::contract::{self, Failure};
use crate::jobs_cli::{consent_of, scan, through_card, Scanned};
use notchling_local_api::{ops, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::time::Duration;

pub const USAGE: &str = "Usage: notchling machines list\n       notchling machines check [<machine>]\n       notchling machines plan <machine> [<step>]\n       notchling machines peers\n       notchling machines add <user@host> [--name <text>]\n       notchling machines import <peer> --user <user>\n       notchling machines addresses <machine> <address>...\n       notchling machines remove <machine>\n       notchling machines fix <machine> <step>\n       notchling machines setup <machine>\n       notchling machines running <machine> on|off\n       notchling machines network switch <tailnet>\n       the changes take [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]; the user says yes on a card";

pub const OWN: [&str; 2] = ["--name", "--user"];

fn on_off(word: &str) -> Result<bool, String> {
    match word {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("Say on or off.\n{USAGE}")),
    }
}

pub fn read_call(words: &[&str]) -> Option<(&'static str, Value)> {
    match words {
        ["list"] => Some((ops::MACHINES_LIST, json!({}))),
        ["check"] => Some((ops::MACHINES_CHECK, json!({}))),
        ["check", machine] => Some((ops::MACHINES_CHECK, json!({"machine": machine}))),
        ["plan", machine] => Some((ops::MACHINES_PLAN, json!({"machine": machine}))),
        ["plan", machine, step] => Some((
            ops::MACHINES_PLAN,
            json!({"action": "fix", "args": {"machine": machine, "step": step}}),
        )),
        ["peers"] => Some((ops::MACHINES_PEERS, json!({}))),
        _ => None,
    }
}

pub fn request(scanned: &Scanned) -> Result<Value, String> {
    let words: Vec<&str> = scanned.words.iter().map(String::as_str).collect();
    let usage = || USAGE.to_string();
    let (action, args) = match words.as_slice() {
        ["add", address] => {
            let mut args = json!({"address": address});
            if let Some(name) = scanned.one("name") {
                args["name"] = json!(name);
            }
            ("add", args)
        }
        ["import", peer] => {
            let user = scanned.one("user").ok_or_else(usage)?;
            ("import", json!({"peer": peer, "user": user}))
        }
        ["addresses", machine, list @ ..] if !list.is_empty() => {
            ("addresses", json!({"machine": machine, "addresses": list}))
        }
        ["remove", machine] => ("remove", json!({"machine": machine})),
        ["fix", machine, step] => ("fix", json!({"machine": machine, "step": step})),
        ["setup", machine] => ("setup", json!({"machine": machine})),
        ["running", machine, state] => {
            ("running", json!({"machine": machine, "on": on_off(state)?}))
        }
        ["network", "switch", tailnet] => ("network", json!({"tailnet": tailnet})),
        _ => return Err(usage()),
    };
    let mut body = json!({"action": action, "args": args, "consent": consent_of(scanned)});
    if scanned.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    Ok(body)
}

pub fn done_line(request: &Value) -> String {
    let action = request
        .pointer("/result/action")
        .and_then(Value::as_str)
        .unwrap_or("change");
    format!("Done: {action} (the user said yes).")
}

pub const CHECK_TIMEOUT: Duration = Duration::from_secs(150);

pub const LISTING_TIMEOUT: Duration = Duration::from_secs(30);

pub fn read_timeout(op: &str) -> Duration {
    if op == ops::MACHINES_CHECK {
        CHECK_TIMEOUT
    } else if op == ops::MACHINES_PEERS || op == ops::NETWORK_LIST {
        LISTING_TIMEOUT
    } else {
        CALL_TIMEOUT
    }
}

pub fn read(scanned: &Scanned, op: &str, args: Value) -> i32 {
    let json = scanned.json;
    let mut client = match contract::connect(scanned.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(read_timeout(op));
    match contract::call(&mut client, op, args) {
        Ok(reply) => {
            println!("{}", contract::pretty(&reply));
            contract::OK
        }
        Err(failure) => failure.report(json),
    }
}

pub(crate) fn run_words(raw: &[String], usage: &str, own: &[&str], prefix: Option<&str>) -> i32 {
    let json = contract::wants_json(raw);
    let mut scanned = match scan(raw, usage, own) {
        Ok(s) => s,
        Err(m) if m == usage && raw.iter().any(|a| a == "--help" || a == "-h") => {
            println!("{m}");
            return contract::OK;
        }
        Err(m) => return Failure::usage(m).report(json),
    };
    if let Some(prefix) = prefix {
        scanned.words.insert(0, prefix.to_string());
    }
    let words: Vec<&str> = scanned.words.iter().map(String::as_str).collect();
    if let Some((op, args)) = read_call(&words) {
        return read(&scanned, op, args);
    }
    if words == ["network", "list"] {
        return read(&scanned, ops::NETWORK_LIST, json!({}));
    }
    let body = match request(&scanned) {
        Ok(b) => b,
        Err(m) => return Failure::usage(m).report(json),
    };
    through_card(ops::MACHINES_REQUEST, body, &scanned, usage, |reply, _| {
        done_line(reply.get("request").unwrap_or(reply))
    })
}

pub fn run(args: &[String]) -> i32 {
    run_words(args, USAGE, &OWN, None)
}

#[cfg(test)]
#[path = "machines_cli_tests.rs"]
mod tests;
