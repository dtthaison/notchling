use crate::contract::{self, Failure};
use crate::jobs_cli::{consent_of, scan, through_card, Scanned};
use notchling_local_api::ops;
use serde_json::{json, Value};

pub const USAGE: &str = "Usage: notchling versions update <tool> [--to <version>] [--worker <machine>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]";
pub const CHECK_USAGE: &str = "Usage: notchling check-updates [on|off] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]";
pub const SETTING: &str = "checkForUpdates";

pub fn request(scanned: &Scanned) -> Result<(&'static str, Value), String> {
    let tool = match scanned.words.as_slice() {
        [tool] => tool.as_str(),
        _ => return Err(USAGE.to_string()),
    };
    let to = scanned.one("to");
    let mut body = json!({"consent": consent_of(scanned)});
    if scanned.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    match scanned.one("worker") {
        Some(machine) => {
            let mut args = json!({"machine": machine, "tool": tool});
            if let Some(to) = to {
                args["to"] = json!(to);
            }
            body["action"] = json!("update");
            body["args"] = args;
            Ok((ops::MACHINES_REQUEST, body))
        }
        None => {
            body["tool"] = json!(tool);
            if let Some(to) = to {
                body["to"] = json!(to);
            }
            Ok((ops::VERSIONS_REQUEST, body))
        }
    }
}

pub fn done_line(request: &Value) -> String {
    let tool = request
        .pointer("/result/tool")
        .and_then(Value::as_str)
        .unwrap_or("the tool");
    let back = request.pointer("/result/action").and_then(Value::as_str) == Some("back");
    if back {
        format!("Done: {tool} goes back (the user said yes). It finishes in the background.")
    } else {
        format!("Done: {tool} updates (the user said yes). It finishes in the background.")
    }
}

pub fn update(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let scanned = match scan(args, USAGE, &["--to", "--worker"]) {
        Ok(s) => s,
        Err(m) => return Failure::usage(m).report(json),
    };
    let (op, body) = match request(&scanned) {
        Ok(found) => found,
        Err(m) => return Failure::usage(m).report(json),
    };
    through_card(op, body, &scanned, USAGE, |reply, _| {
        done_line(reply.get("request").unwrap_or(reply))
    })
}

pub fn settings_args(args: &[String]) -> Result<Vec<String>, String> {
    let mut words: Vec<&String> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    let mut flags = args.iter();
    while let Some(arg) = flags.next() {
        if arg.starts_with("--") {
            rest.push(arg.clone());
            if matches!(
                arg.as_str(),
                "--reason" | "--timeout" | "--expires-in" | "--key" | "--socket"
            ) {
                rest.extend(flags.next().cloned());
            }
        } else {
            words.push(arg);
        }
    }
    let mut out = vec![SETTING.to_string()];
    let first = match words.as_slice() {
        [] => "get",
        [word] if word.as_str() == "on" || word.as_str() == "off" => {
            out.push(word.to_string());
            "set"
        }
        _ => return Err(CHECK_USAGE.to_string()),
    };
    out.insert(0, first.to_string());
    out.extend(rest);
    Ok(out)
}

pub fn check_updates(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Failure::usage(CHECK_USAGE.to_string()).report(json);
    }
    match settings_args(args) {
        Ok(forwarded) => crate::settings_cli::run(&forwarded),
        Err(m) => Failure::usage(m).report(json),
    }
}

#[cfg(test)]
#[path = "versions_cli_tests.rs"]
mod tests;
