use crate::contract::{self, Failure};
use crate::jobs_cli::{consent_of, scan, through_card, Scanned};
use notchling_engine::install::{GUARD_HOOKS, PAGE_HOOKS, PROMPT_HOOKS};
use notchling_local_api::ops;
use serde_json::{json, Map, Value};
use std::io::Write;

pub const INSTALL_USAGE: &str = "Usage: notchling install plan [--what <a,b>] [--json] [--socket <path>]\n       notchling install request [--part <id>]... [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]\n       plan only reads and changes nothing; a part is hooks, sidecar, a skill's name or skill:<tool>:<name>; left out it is every file Set Up would write; the user says yes on a card and may keep only some";
pub const COST_USAGE: &str = "Usage: notchling cost profile [--json]\n       notchling cost profile set [--preset <name>] [--tool <claude|codex|agy>=on|off]... [--pick <role.tool>=<model effort>]... [--habit <name>=<number>]... [--reset]\n       notchling cost profile setup [--consent <a,b>]\n       notchling cost profile undo <claude|codex|agy>\n       each change takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]; the user says yes on a card and may keep only some of the files";

fn names_of(part: &str) -> Vec<String> {
    match part {
        "hooks" => vec![PROMPT_HOOKS.into(), PAGE_HOOKS.into(), GUARD_HOOKS.into()],
        other => match other.strip_prefix("skill:") {
            Some(rest) => vec![rest.rsplit(':').next().unwrap_or(rest).to_string()],
            None => vec![other.to_string()],
        },
    }
}

pub fn plan_body(scanned: &Scanned) -> Result<Value, String> {
    match scanned.words.as_slice() {
        [word] if word == "plan" && scanned.all("part").is_empty() => {}
        _ => return Err(INSTALL_USAGE.to_string()),
    }
    let mut body = json!({});
    if let Some(list) = scanned.one("what") {
        let names: Vec<&str> = list.split(',').filter(|n| !n.is_empty()).collect();
        body["what"] = json!(names);
    }
    Ok(body)
}

pub fn install_body(scanned: &Scanned) -> Result<Value, String> {
    match scanned.words.as_slice() {
        [word] if word == "request" && scanned.one("what").is_none() => {}
        _ => return Err(INSTALL_USAGE.to_string()),
    }
    let mut parts: Vec<String> = Vec::new();
    for part in scanned.all("part") {
        for name in names_of(part) {
            if !parts.contains(&name) {
                parts.push(name);
            }
        }
    }
    let mut body = json!({"consent": consent_of(scanned)});
    if !parts.is_empty() {
        body["parts"] = json!(parts);
    }
    if scanned.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    Ok(body)
}

fn pair<'a>(text: &'a str, what: &str) -> Result<(&'a str, &'a str), String> {
    text.split_once('=')
        .filter(|(k, _)| !k.is_empty())
        .ok_or_else(|| format!("{what} is name=value.\n{COST_USAGE}"))
}

fn on_off(word: &str) -> Result<bool, String> {
    match word {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("Say on or off.\n{COST_USAGE}")),
    }
}

fn profile_args(scanned: &Scanned, reset: bool) -> Result<Value, String> {
    let mut args = Map::new();
    if let Some(name) = scanned.one("preset") {
        args.insert("preset".into(), json!(name));
    }
    let mut tools = Map::new();
    for text in scanned.all("tool") {
        let (tool, on) = pair(text, "--tool")?;
        tools.insert(tool.to_string(), json!(on_off(on)?));
    }
    if !tools.is_empty() {
        args.insert("tools".into(), Value::Object(tools));
    }
    let mut picks = Map::new();
    for text in scanned.all("pick") {
        let (key, cell) = pair(text, "--pick")?;
        let value = if cell.is_empty() || cell == "default" {
            Value::Null
        } else {
            json!(cell)
        };
        picks.insert(key.to_string(), value);
    }
    if !picks.is_empty() {
        args.insert("picks".into(), Value::Object(picks));
    }
    let mut habits = Map::new();
    for text in scanned.all("habit") {
        let (key, n) = pair(text, "--habit")?;
        let value = if n == "default" {
            Value::Null
        } else {
            json!(n
                .parse::<u64>()
                .map_err(|_| format!("{key} is a whole number.\n{COST_USAGE}"))?)
        };
        habits.insert(key.to_string(), value);
    }
    if !habits.is_empty() {
        args.insert("habits".into(), Value::Object(habits));
    }
    if reset {
        args.insert("reset".into(), json!(true));
    }
    Ok(Value::Object(args))
}

pub fn cost_body(scanned: &Scanned, reset: bool) -> Result<Value, String> {
    let words: Vec<&str> = scanned.words.iter().map(String::as_str).collect();
    let (action, args) = match words.as_slice() {
        ["set"] => ("profile", profile_args(scanned, reset)?),
        ["setup"] => {
            let mut args = json!({});
            if let Some(list) = scanned.one("consent") {
                let names: Vec<&str> = list.split(',').filter(|n| !n.is_empty()).collect();
                args["consent"] = json!(names);
            }
            ("apply", args)
        }
        ["undo", tool] => ("undo", json!({"tool": tool})),
        _ => return Err(COST_USAGE.to_string()),
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
    let states = request
        .pointer("/result/parts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let of = |state: &str| -> Vec<&str> {
        states
            .iter()
            .filter(|p| p["state"] == state)
            .filter_map(|p| p["id"].as_str())
            .collect()
    };
    let mut line = format!("Done: {action} (the user said yes).");
    for state in ["written", "stale", "failed"] {
        let ids = of(state);
        if !ids.is_empty() {
            line.push_str(&format!("\n{state}: {}", ids.join(", ")));
        }
    }
    if !of("stale").is_empty() {
        line.push_str("\nA stale file changed after the card; it was left as it is. Ask again.");
    }
    line
}

fn bail(message: String, raw: &[String]) -> i32 {
    if message.starts_with("Usage:") && raw.iter().any(|a| a == "--help" || a == "-h") {
        let _ = writeln!(std::io::stdout(), "{message}");
        return contract::OK;
    }
    Failure::usage(message).report(contract::wants_json(raw))
}

pub fn run(raw: &[String]) -> i32 {
    let scanned = match scan(raw, INSTALL_USAGE, &["--part", "--what"]) {
        Ok(s) => s,
        Err(m) => return bail(m, raw),
    };
    if scanned.words.first().is_some_and(|w| w == "plan") {
        return match plan_body(&scanned) {
            Ok(body) => read_op(ops::INSTALL_PLAN, body, &scanned, contract::wants_json(raw)),
            Err(m) => bail(m, raw),
        };
    }
    let body = match install_body(&scanned) {
        Ok(b) => b,
        Err(m) => return bail(m, raw),
    };
    through_card(
        ops::INSTALL_REQUEST,
        body,
        &scanned,
        INSTALL_USAGE,
        |reply, _| done_line(reply.get("request").unwrap_or(reply)),
    )
}

pub fn cost_profile(raw: &[String]) -> i32 {
    let reset = raw.iter().any(|a| a == "--reset");
    let rest: Vec<String> = raw.iter().filter(|a| *a != "--reset").cloned().collect();
    let json = contract::wants_json(raw);
    let scanned = match scan(
        &rest,
        COST_USAGE,
        &["--preset", "--tool", "--pick", "--habit", "--consent"],
    ) {
        Ok(s) => s,
        Err(m) => return bail(m, raw),
    };
    if scanned.words.is_empty() && !reset {
        return read_op(ops::COST_PROFILE_GET, json!({}), &scanned, json);
    }
    let body = match cost_body(&scanned, reset) {
        Ok(b) => b,
        Err(m) => return bail(m, raw),
    };
    through_card(ops::COST_REQUEST, body, &scanned, COST_USAGE, |reply, _| {
        done_line(reply.get("request").unwrap_or(reply))
    })
}

fn read_op(op: &str, body: Value, scanned: &Scanned, json: bool) -> i32 {
    let mut client = match contract::connect(scanned.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    match contract::call(&mut client, op, body) {
        Ok(reply) => {
            let _ = writeln!(std::io::stdout(), "{}", contract::pretty(&reply));
            contract::OK
        }
        Err(failure) => failure.report(json),
    }
}

#[cfg(test)]
#[path = "install_cli_tests.rs"]
mod tests;
