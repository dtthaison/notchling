use crate::contract::{self, Failure};
use crate::jobs_cli::{consent_of, scan, through_card, Scanned};
use notchling_local_api::{ops, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::io::Write;

pub const USAGE: &str = "Usage: notchling accounts switch <account>\n       notchling accounts add claude|codex|agy\n       notchling accounts remove <account>\n       notchling accounts move-all <account>|main\n       notchling accounts move <session> <lane>|main\n       notchling lanes list [--json]\n       notchling lanes create <account>\n       notchling lanes remove <lane>\n       notchling lanes switch <lane> <account>\n       each change takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]; the user says yes on a card";

pub const WORDS: [&str; 5] = ["switch", "add", "remove", "move-all", "move"];

pub fn handles(word: &str) -> bool {
    WORDS.contains(&word)
}

fn usage() -> String {
    USAGE.to_string()
}

fn finish(action: &str, args: Value, scanned: &Scanned) -> Value {
    let mut body = json!({"action": action, "args": args, "consent": consent_of(scanned)});
    if scanned.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    body
}

pub fn request(scanned: &Scanned) -> Result<Value, String> {
    let words: Vec<&str> = scanned.words.iter().map(String::as_str).collect();
    let (action, args) = match words.as_slice() {
        ["switch", account] => ("switch", json!({"account": account})),
        ["add", tool] => {
            if !matches!(*tool, "claude" | "codex" | "agy") {
                return Err(format!("The tool is claude, codex or agy.\n{USAGE}"));
            }
            ("add", json!({"tool": tool}))
        }
        ["remove", account] => ("remove", json!({"account": account})),
        ["move-all", to] => ("move_all", json!({"to": to})),
        ["move", session, to] => ("move", json!({"session": session, "to": to})),
        _ => return Err(usage()),
    };
    Ok(finish(action, args, scanned))
}

pub fn lanes_request(scanned: &Scanned) -> Result<Value, String> {
    let words: Vec<&str> = scanned.words.iter().map(String::as_str).collect();
    let (action, args) = match words.as_slice() {
        ["create", account] => ("lane_create", json!({"account": account})),
        ["remove", lane] => ("lane_remove", json!({"lane": lane})),
        ["switch", lane, account] => ("lane_switch", json!({"lane": lane, "account": account})),
        _ => return Err(usage()),
    };
    Ok(finish(action, args, scanned))
}

fn words_of(action: &str) -> &'static str {
    match action {
        "switch" => "accounts switch",
        "add" => "accounts add",
        "remove" => "accounts remove",
        "move_all" => "accounts move-all",
        "move" => "accounts move",
        "lane_create" => "lanes create",
        "lane_remove" => "lanes remove",
        "lane_switch" => "lanes switch",
        _ => "accounts",
    }
}

pub fn done_line(request: &Value) -> String {
    let action = request
        .pointer("/result/action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut line = format!("Done: {} (the user said yes).", words_of(action));
    if let Some(account) = request.pointer("/result/account").and_then(Value::as_str) {
        if action == "add" {
            line.push_str(&format!("\nNew account: {account}"));
        }
    }
    if action == "move_all" {
        if let Some(moved) = request.pointer("/result/moved").filter(|v| !v.is_null()) {
            let errors = request
                .pointer("/result/errors")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or(json!(0));
            line.push_str(&format!("\nMoved: {moved}, errors: {errors}"));
        }
    }
    if let Some(lane) = request.pointer("/result/lane").and_then(Value::as_str) {
        line.push_str(&format!("\nLane: {lane}"));
    }
    line
}

fn change(raw: &[String], build: fn(&Scanned) -> Result<Value, String>) -> i32 {
    let json = contract::wants_json(raw);
    let scanned = match scan(raw, USAGE, &[]) {
        Ok(s) => s,
        Err(m) if m == USAGE && raw.iter().any(|a| a == "--help" || a == "-h") => {
            println!("{m}");
            return contract::OK;
        }
        Err(m) => return Failure::usage(m).report(json),
    };
    let body = match build(&scanned) {
        Ok(b) => b,
        Err(m) => return Failure::usage(m).report(json),
    };
    through_card(ops::ACCOUNTS_REQUEST, body, &scanned, USAGE, |reply, _| {
        done_line(reply.get("request").unwrap_or(reply))
    })
}

pub fn run(raw: &[String]) -> i32 {
    change(raw, request)
}

pub fn lanes_text(reply: &Value) -> String {
    contract::pretty(reply)
}

pub fn lanes_list(
    raw: &[String],
    mut ask: impl FnMut(Option<&std::path::Path>, &str, Value) -> Result<Value, Failure>,
    out: &mut impl Write,
) -> i32 {
    let json = contract::wants_json(raw);
    let scanned = match scan(raw, USAGE, &[]) {
        Ok(s) => s,
        Err(m) if m == USAGE && raw.iter().any(|a| a == "--help" || a == "-h") => {
            let _ = writeln!(out, "{m}");
            return contract::OK;
        }
        Err(m) => return Failure::usage(m).report(json),
    };
    if scanned.words.as_slice() != ["list"] || scanned.flags.dry_run {
        return Failure::usage(usage()).report(json);
    }
    match ask(scanned.socket.as_deref(), ops::LANES_SUMMARY, json!({})) {
        Ok(reply) => {
            let _ = writeln!(out, "{}", lanes_text(&reply));
            contract::OK
        }
        Err(failure) => failure.report(json),
    }
}

pub fn run_lanes(raw: &[String]) -> i32 {
    if raw.first().map(String::as_str) == Some("list") {
        return lanes_list(
            raw,
            |socket, op, args| {
                let mut client = contract::connect(socket)?;
                client.set_timeout(CALL_TIMEOUT);
                contract::call(&mut client, op, args)
            },
            &mut std::io::stdout(),
        );
    }
    change(raw, lanes_request)
}

#[cfg(test)]
#[path = "accounts_cli_tests.rs"]
mod tests;
