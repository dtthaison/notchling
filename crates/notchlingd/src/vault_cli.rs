use crate::contract::{self, Failure};
use notchling_engine::vault::list::describe;
use notchling_engine::vault::{expiry, runner, Kind, VaultEntry, VaultRequest};
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

const USE_TIMEOUT: Duration = Duration::from_secs(300 + 15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(540 + 15);

pub const REQUEST_USAGE: &str = "Usage: notchling secret-request <name> [--kind token|login] [--url <url>] [--note <what for>] [--how <steps to get or renew it>] [--expires YYYY-MM-DD] [--reason <why now>]";

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
    let lead = args.iter().take_while(|a| *a == "--json").count();
    let args = &args[lead..];
    let Some(name) = args.first().filter(|a| !a.starts_with("--")) else {
        return Err(REQUEST_USAGE.to_string());
    };
    let mut request = VaultRequest {
        name: name.clone(),
        ..VaultRequest::default()
    };
    let mut socket = None;
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        if flag == "--json" {
            continue;
        }
        let value = rest.next().cloned();
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
    Ok((request, socket))
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
    let (request, socket) = match parse_request(args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let args = serde_json::to_value(&request).unwrap_or(Value::Null);
    match ask(socket.as_deref(), ops::VAULT_REQUEST, args, REQUEST_TIMEOUT) {
        Ok(reply) => match reply.get("entry").cloned().map(serde_json::from_value) {
            Some(Ok(entry)) if json => {
                let shown = json!({"entry": entry, "request": reply.get("request").cloned()});
                println!("{}", contract::pretty(&shown));
                0
            }
            Some(Ok(entry)) => {
                let mut out = std::io::stdout();
                let _ = writeln!(out, "{}", saved_report(&entry, today()));
                0
            }
            _ => Failure::new(contract::FAILED, "failed", "Notchling didn't answer.")
                .with_op(ops::VAULT_REQUEST)
                .report(json),
        },
        Err(failure) => failure.report(json),
    }
}

#[cfg(test)]
#[path = "vault_cli_tests.rs"]
mod tests;
