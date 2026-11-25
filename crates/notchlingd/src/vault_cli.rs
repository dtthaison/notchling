use crate::contract::{self, Failure};
use crate::settings_cli::Flags;
use notchling_engine::vault::list::describe;
use notchling_engine::vault::naming;
use notchling_engine::vault::{expiry, runner, Kind, VaultEntry, VaultRequest};
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

const USE_TIMEOUT: Duration = Duration::from_secs(300 + 15);
const CALL_WAIT: Duration = Duration::from_secs(15);
const POLL_MS: u64 = 25_000;

pub const REQUEST_USAGE: &str = "Usage: notchling secret-request <name> [--kind token|login] [--url <url>] [--note <what for>] [--how <steps to get or renew it>] [--expires YYYY-MM-DD] [--reason <why now>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run]";

pub fn usage() -> String {
    format!(
        "Usage: notchling secret <name>[,<name>…] -- <command> [args…]   (one argument runs through sh -c)\n       notchling secrets [url | host | words]\n       {}",
        REQUEST_USAGE.trim_start_matches("Usage: ")
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UseArgs {
    pub names: Vec<String>,
    pub command: Vec<String>,
    pub socket: Option<PathBuf>,
}

pub fn parse_use(args: &[String]) -> Result<UseArgs, String> {
    let mut first: Option<String> = None;
    let mut socket = None;
    let mut rest = args.iter();
    loop {
        match rest.next().map(String::as_str) {
            Some("--") => break,
            Some("--json") => {}
            Some("--socket") => socket = rest.next().map(PathBuf::from),
            Some(word) if !word.starts_with("--") && first.is_none() => {
                first = Some(word.to_string());
            }
            _ => return Err(usage()),
        }
    }
    let Some(first) = first else {
        return Err(usage());
    };
    let names: Vec<String> = first
        .split(',')
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    let command: Vec<String> = rest.cloned().collect();
    if names.is_empty() || command.is_empty() {
        return Err(usage());
    }
    Ok(UseArgs {
        names,
        command,
        socket,
    })
}

pub fn parse_request(args: &[String]) -> Result<(VaultRequest, Option<PathBuf>), String> {
    parse_request_flags(args).map(|(request, socket, _)| (request, socket))
}

pub fn parse_request_flags(
    args: &[String],
) -> Result<(VaultRequest, Option<PathBuf>, Flags), String> {
    let mut name: Option<String> = None;
    let mut request = VaultRequest::default();
    let mut socket = None;
    let mut flags = Flags::default();
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--help" | "-h" => return Err(REQUEST_USAGE.to_string()),
            "--json" => continue,
            "--wait" => {
                flags.wait = true;
                continue;
            }
            "--no-wait" => {
                flags.wait = false;
                continue;
            }
            "--dry-run" => {
                flags.dry_run = true;
                continue;
            }
            _ => {}
        }
        if !flag.starts_with("--") {
            if name.is_some() {
                return Err(format!("Unexpected {flag}.\n{REQUEST_USAGE}"));
            }
            name = Some(flag.clone());
            continue;
        }
        let value = rest.next().cloned();
        let seconds = |value: &Option<String>| -> Result<u64, String> {
            value
                .as_deref()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| format!("{flag} takes seconds.\n{REQUEST_USAGE}"))
        };
        match flag.as_str() {
            "--timeout" => {
                flags.timeout = Duration::from_secs(seconds(&value)?);
                continue;
            }
            "--expires-in" => {
                flags.expires_in = Some(seconds(&value)?);
                continue;
            }
            "--key" => {
                flags.key =
                    Some(value.ok_or_else(|| format!("--key needs a value.\n{REQUEST_USAGE}"))?);
                continue;
            }
            _ => {}
        }
        let slot = match flag.as_str() {
            "--kind" => &mut request.kind,
            "--url" => &mut request.url,
            "--note" => &mut request.note,
            "--how" => &mut request.how_to,
            "--expires" => &mut request.expires,
            "--reason" => &mut request.reason,
            "--socket" => {
                socket = value.map(PathBuf::from);
                continue;
            }
            other => return Err(format!("Unknown option {other}.\n{REQUEST_USAGE}")),
        };
        match value {
            Some(v) => *slot = Some(v),
            None if flag == "--expires" => {
                return Err("--expires takes a day as YYYY-MM-DD, like 2026-12-06.".into())
            }
            None => return Err(REQUEST_USAGE.to_string()),
        }
    }
    let Some(name) = name else {
        return Err(REQUEST_USAGE.to_string());
    };
    request.name = name;
    if request
        .kind
        .as_deref()
        .is_some_and(|k| Kind::parse(k).is_none())
    {
        return Err("--kind is token or login.".into());
    }
    if let Some(given) = request.expires.clone() {
        match expiry::parse(&given) {
            Some(day) => request.expires = Some(day),
            None => {
                return Err(format!(
                    "--expires takes a day as YYYY-MM-DD, like 2026-12-06 (got \"{given}\")."
                ))
            }
        }
    }
    Ok((request, socket, flags))
}

pub fn parse_find(args: &[String]) -> (String, Option<PathBuf>, bool) {
    let mut words = Vec::new();
    let mut socket = None;
    let mut json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--socket" {
            socket = rest.next().map(PathBuf::from);
        } else if arg == "--json" {
            json = true;
        } else {
            words.push(arg.clone());
        }
    }
    (words.join(" "), socket, json)
}

pub fn shown(command: &[String]) -> String {
    if let [one] = command {
        return one.clone();
    }
    let quoted = |a: &String| {
        let plain = !a.is_empty()
            && a.chars()
                .all(|c| c.is_ascii_alphanumeric() || "_./:=@%+-".contains(c));
        if plain {
            a.clone()
        } else {
            format!("'{}'", a.replace('\'', r"'\''"))
        }
    };
    command.iter().map(quoted).collect::<Vec<_>>().join(" ")
}

fn entries(reply: &Value, key: &str) -> Vec<VaultEntry> {
    reply
        .get(key)
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn find_report(reply: &Value, query: &str, today: i64) -> (i32, String) {
    let saved: Vec<String> = reply
        .get("saved")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    if saved.is_empty() {
        return (1, "No secrets saved yet. Ask the user for one with secret-request <name> --kind token|login --url <url> --note <what for> --reason <why>.".into());
    }
    let shown = entries(reply, "entries");
    if shown.is_empty() {
        return (
            1,
            format!(
                "Nothing saved fits \"{query}\". Saved: {}.\nIf none of them is the one, ask the user with secret-request.",
                saved.join(", ")
            ),
        );
    }
    let text: Vec<String> = shown.iter().map(|e| describe(e, today)).collect();
    (0, text.join("\n"))
}

pub fn find_json(reply: &Value, query: &str) -> (i32, Value) {
    let saved: Vec<String> = reply
        .get("saved")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let shown = entries(reply, "entries");
    let found = json!({"query": query, "saved": saved, "entries": shown});
    let message = if saved.is_empty() {
        "No secrets are saved yet."
    } else if shown.is_empty() {
        "Nothing saved fits that name."
    } else {
        return (contract::OK, found);
    };
    let failure = Failure::failed(message).with_op(ops::VAULT_FIND);
    (failure.exit, failure.to_value_with(found))
}

pub fn saved_report(entry: &VaultEntry, today: i64) -> String {
    let vars: Vec<String> = entry
        .all_variables()
        .iter()
        .map(|v| format!("${v}"))
        .collect();
    format!(
        "Saved.\n{}\nUse it: notchling secret {} -- '<command using {}>'",
        describe(entry, today),
        entry.name,
        vars.join(", ")
    )
}

pub fn renamed_line(asked: &str, saved: &str) -> Option<String> {
    (asked != saved)
        .then(|| format!("The user saved it as {saved}, not {asked}. Use the name {saved}."))
}

fn today() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    expiry::today_local(now)
}

fn ask(
    socket: Option<&std::path::Path>,
    op: &str,
    args: Value,
    timeout: Duration,
) -> Result<Value, Failure> {
    let mut client = contract::connect(socket)?;
    client.set_timeout(timeout);
    contract::call(&mut client, op, args)
}

pub fn secrets(args: &[String]) -> i32 {
    let (query, socket, json) = parse_find(args);
    let reply = ask(
        socket.as_deref(),
        ops::VAULT_FIND,
        json!({"query": query}),
        notchling_local_api::CALL_TIMEOUT,
    );
    match reply {
        Ok(reply) => {
            if json {
                let (code, value) = find_json(&reply, &query);
                println!("{}", contract::pretty(&value));
                return code;
            }
            let (code, text) = find_report(&reply, &query, today());
            println!("{text}");
            code
        }
        Err(failure) => failure.report(json),
    }
}

fn stopped(failure: Failure, json: bool) -> i32 {
    let mut failure = failure;
    failure.exit = contract::STOPPED;
    failure.code = "stopped".to_string();
    let line = if json {
        failure.to_value().to_string()
    } else {
        failure.line()
    };
    eprintln!("{line}");
    failure.exit
}

pub fn secret(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let parsed = match parse_use(args) {
        Ok(p) => p,
        Err(message) => return stopped(Failure::usage(message), json),
    };
    let folder = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let args = json!({"names": parsed.names, "command": shown(&parsed.command), "folder": folder});
    let reply = match ask(parsed.socket.as_deref(), ops::VAULT_USE, args, USE_TIMEOUT) {
        Ok(reply) => reply,
        Err(failure) => return stopped(failure, json),
    };
    let map = |key: &str| -> BTreeMap<String, String> {
        reply
            .get(key)
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    };
    let (env, hide) = (map("env"), map("hide"));
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    runner::run(&parsed.command, &env, &hide, &mut out, &mut err)
}

pub fn secret_request(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let (request, socket, flags) = match parse_request_flags(args) {
        Ok(p) => p,
        Err(message)
            if message == REQUEST_USAGE && args.iter().any(|a| a == "--help" || a == "-h") =>
        {
            if json {
                println!(
                    "{}",
                    contract::pretty(&json!({"usage": REQUEST_USAGE, "naming": naming::RULE}))
                );
            } else {
                println!("{REQUEST_USAGE}\n\n{}", naming::RULE);
            }
            return contract::OK;
        }
        Err(message) => return Failure::usage(message).report(json),
    };
    let asked = request.name.clone();
    let mut body = serde_json::to_value(&request).unwrap_or(Value::Null);
    let mut consent = serde_json::Map::new();
    if let Some(seconds) = flags.expires_in {
        consent.insert("expiresInMs".into(), json!(seconds.saturating_mul(1000)));
    }
    if let Some(key) = &flags.key {
        consent.insert("key".into(), json!(key));
    }
    body["consent"] = Value::Object(consent);
    if flags.dry_run {
        body["dryRun"] = json!(true);
    }
    let end = match contract::deadline(flags.timeout, REQUEST_USAGE) {
        Ok(end) => end,
        Err(failure) => return failure.report(json),
    };
    let mut client = match contract::connect(socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(CALL_WAIT);
    let first = match contract::call(&mut client, ops::VAULT_REQUEST, body) {
        Ok(reply) => reply,
        Err(failure) => return failure.report(json),
    };
    if flags.dry_run {
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
    let request = match contract::wait_while_pending(request, flags.wait, end, || {
        contract::call_through_restart(&mut client, socket.as_deref(), ops::CONSENT_GET, &ask, end)
    }) {
        Ok(request) => request,
        Err(failure) => return failure.report(json),
    };
    if state_of(&request) != "applied" {
        return ended(&request, &asked).report(json);
    }
    let entry = request
        .get("result")
        .and_then(|r| r.get("entry"))
        .cloned()
        .map(serde_json::from_value::<VaultEntry>);
    match entry {
        Some(Ok(entry)) if json => {
            let shown = json!({"entry": entry, "savedName": entry.name,
                               "requestedName": request.get("target"), "request": request});
            println!("{}", contract::pretty(&shown));
            contract::OK
        }
        Some(Ok(entry)) => {
            let mut out = std::io::stdout();
            if let Some(line) = renamed_line(&asked, &entry.name) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out, "{}", saved_report(&entry, today()));
            contract::OK
        }
        _ => Failure::new(contract::FAILED, "failed", "Notchling didn't answer.")
            .with_op(ops::VAULT_REQUEST)
            .report(json),
    }
}

pub fn ended(request: &Value, asked: &str) -> Failure {
    let mut failure = crate::consent_cli::failure_of(request, ops::VAULT_REQUEST);
    if state_of(request) == "denied" {
        let message =
            format!("The user declined to save {asked}. Don't ask again until they tell you why.");
        failure.human = Some(format!("{} ({message})", crate::consent_cli::line(request)));
        failure.message = message;
    }
    failure
}

fn state_of(request: &Value) -> &str {
    request
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("failed")
}

#[cfg(test)]
#[path = "vault_cli_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "vault_cli_declined_tests.rs"]
mod declined_tests;
