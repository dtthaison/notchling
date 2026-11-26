use crate::contract::{self, Failure};
use crate::jobs_cli::{consent_of, scan, through_card, Scanned};
use notchling_local_api::ops;
use serde_json::{json, Value};

pub const USAGE: &str = "Usage: notchling cluster create <name>\n       notchling cluster token\n       notchling cluster invite --node <id> --name <name> [--role master|worker] [--acting on|off]\n       notchling cluster member <node> [--role master|worker] [--delegation on|off]\n       notchling cluster failover on|off|30m|1h\n       notchling cluster main <session> [on|off]\n       notchling cluster allow <node> <secret>...\n       notchling cluster remove <node>\n       notchling cluster move-home <node>\n       notchling cluster move-now\n       each takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]; the user says yes on a card";

pub const WORDS: [&str; 10] = [
    "create",
    "token",
    "invite",
    "member",
    "failover",
    "main",
    "allow",
    "remove",
    "move-home",
    "move-now",
];

pub fn handles(word: &str) -> bool {
    WORDS.contains(&word)
}

fn usage() -> String {
    USAGE.to_string()
}

fn on_off(word: &str) -> Result<bool, String> {
    match word {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("Say on or off.\n{USAGE}")),
    }
}

fn role(word: &str) -> Result<bool, String> {
    match word {
        "master" => Ok(true),
        "worker" => Ok(false),
        _ => Err(format!("The role is master or worker.\n{USAGE}")),
    }
}

pub fn request(scanned: &Scanned) -> Result<Value, String> {
    let words: Vec<&str> = scanned.words.iter().map(String::as_str).collect();
    let (action, args) = match words.as_slice() {
        ["create", name] => ("create", json!({"name": name})),
        ["token"] => ("token", json!({})),
        ["invite"] => {
            let node = scanned.one("node").ok_or_else(usage)?;
            let name = scanned.one("name").ok_or_else(usage)?;
            let mut args = json!({"node": node, "name": name});
            if let Some(r) = scanned.one("role") {
                args["canBeMaster"] = json!(role(r)?);
            }
            if let Some(a) = scanned.one("acting") {
                args["acting"] = json!(on_off(a)?);
            }
            ("invite", args)
        }
        ["member", node] => {
            let mut args = json!({"node": node});
            if let Some(r) = scanned.one("role") {
                role(r)?;
                args["role"] = json!(r);
            }
            if let Some(d) = scanned.one("delegation") {
                args["delegation"] = json!(on_off(d)?);
            }
            if args.as_object().is_some_and(|m| m.len() < 2) {
                return Err(usage());
            }
            ("member", args)
        }
        ["failover", value] => ("failover", json!({"value": value})),
        ["main", session] => ("main", json!({"session": session})),
        ["main", session, state] => ("main", json!({"session": session, "on": on_off(state)?})),
        ["allow", node, names @ ..] => ("allow", json!({"node": node, "names": names})),
        ["remove", node] => ("remove", json!({"node": node})),
        ["move-home", node] => ("move_home", json!({"node": node})),
        ["move-now"] => ("move_now", json!({})),
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
    let mut line = format!("Done: {action} (the user said yes).");
    if let Some(token) = request.pointer("/handover/token").and_then(Value::as_str) {
        line.push_str(&format!("\nJoin token, shown once: {token}"));
    }
    line
}

pub fn run(raw: &[String]) -> i32 {
    let json = contract::wants_json(raw);
    let scanned = match scan(
        raw,
        USAGE,
        &["--node", "--name", "--role", "--acting", "--delegation"],
    ) {
        Ok(s) => s,
        Err(m) if m == USAGE && raw.iter().any(|a| a == "--help" || a == "-h") => {
            println!("{m}");
            return contract::OK;
        }
        Err(m) => return Failure::usage(m).report(json),
    };
    let body = match request(&scanned) {
        Ok(b) => b,
        Err(m) => return Failure::usage(m).report(json),
    };
    through_card(
        ops::CLUSTER_ADMIN_REQUEST,
        body,
        &scanned,
        USAGE,
        |reply, _| done_line(reply.get("request").unwrap_or(reply)),
    )
}

#[cfg(test)]
#[path = "cluster_admin_cli_tests.rs"]
mod tests;
