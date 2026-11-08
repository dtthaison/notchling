//! `notchling secrets [words]`, `notchling secret <a,b> -- <command…>` and `notchling
//! secret-request <name> [--kind --url --note --how --expires YYYY-MM-DD --reason]`: the Swift
//! app's `--secrets`, `--secret` and `--secret-request`, run by agents. Each also takes
//! `--socket <path>` (before `--` for `secret`).
//!
//! None of them touches the Keychain or a vault file: they ask the daemon's vault service, which
//! answers only while the `vault` switch is on. Output and exit codes are the Swift command's:
//!
//! - `secrets`: the entries that fit, best first (0); none saved or none fits (1).
//! - `secret`: the command's own exit code; the user denied it (3); anything else (2). One
//!   argument after `--` runs through `/bin/sh -c`. Output comes back with values hidden.
//! - `secret-request`: `Saved.`, the entry and how to use it (0); the user cancelled (1);
//!   anything else (2).
//!
//! One difference from the Swift text: it names `notchling secret`/`secret-request` where the
//! Swift text says `Notchling --secret`/`--secret-request`.

use crate::layout::Layout;
use notchling_engine::vault::list::describe;
use notchling_engine::vault::{expiry, runner, Kind, VaultEntry, VaultRequest};
use notchling_local_api::{ops, Client, ClientError, ClientKind, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// How long `secret` and `secret-request` wait for the daemon: past its own waits for the user.
const USE_TIMEOUT: Duration = Duration::from_secs(300 + 15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(540 + 15);

pub const REQUEST_USAGE: &str = "Usage: notchling secret-request <name> [--kind token|login] [--url <url>] [--note <what for>] [--how <steps to get or renew it>] [--expires YYYY-MM-DD] [--reason <why now>]";

/// The three commands' usage, for a `secret` that can't run.
pub fn usage() -> String {
    format!(
        "Usage: notchling secret <name>[,<name>…] -- <command> [args…]   (one argument runs through sh -c)\n       notchling secrets [url | host | words]\n       {}",
        REQUEST_USAGE.trim_start_matches("Usage: ")
    )
}

/// `secret`'s arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UseArgs {
    pub names: Vec<String>,
    pub command: Vec<String>,
    pub socket: Option<PathBuf>,
}

pub fn parse_use(args: &[String]) -> Result<UseArgs, String> {
    let Some(first) = args.first().filter(|a| !a.starts_with("--")) else {
        return Err(usage());
    };
    let names: Vec<String> = first
        .split(',')
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    let mut socket = None;
    let mut rest = args[1..].iter();
    loop {
        match rest.next().map(String::as_str) {
            Some("--") => break,
            Some("--socket") => socket = rest.next().map(PathBuf::from),
            _ => return Err(usage()),
        }
    }
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

/// `secret-request`'s arguments: the request and the socket. `--note` and `--how` keep their
/// newlines.
pub fn parse_request(args: &[String]) -> Result<(VaultRequest, Option<PathBuf>), String> {
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

/// `secrets`' words and socket.
pub fn parse_find(args: &[String]) -> (String, Option<PathBuf>) {
    let mut words = Vec::new();
    let mut socket = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--socket" {
            socket = rest.next().map(PathBuf::from);
        } else {
            words.push(arg.clone());
        }
    }
    (words.join(" "), socket)
}

/// The command as the approval shows it: one argument as it is, several quoted for a shell.
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

/// What `secrets` prints for the daemon's `vault.find` reply: (exit code, stdout).
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

/// What `secret-request` prints for a saved entry.
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

/// One call to the vault service; the error is the text to print and the exit code.
fn ask(
    socket: Option<&std::path::Path>,
    op: &str,
    args: Value,
    timeout: Duration,
    said_no: i32,
) -> Result<Value, (String, i32)> {
    let layout = Layout::from_env(socket).ok_or_else(|| {
        (
            "notchling: no home folder in the environment, so no data folder".to_string(),
            2,
        )
    })?;
    let not_running = || {
        (
            "Notchling isn't running. Ask the user to open it, then try again.".to_string(),
            2,
        )
    };
    let mut client =
        match Client::connect(&layout.socket, ClientKind::Cli, env!("CARGO_PKG_VERSION")) {
            Ok(c) => c,
            Err(ClientError::NotRunning(_) | ClientError::Unsupported) => return Err(not_running()),
            Err(e) => return Err((format!("notchling: {e}"), 2)),
        };
    client.set_timeout(timeout);
    client.call(op, args).map_err(|e| match e {
        ClientError::Api(e) if e.code == ErrorCode::NeedsApproval => (e.message, said_no),
        ClientError::Api(e) => (e.message, 2),
        ClientError::Timeout => ("Notchling didn't answer.".to_string(), 2),
        other => (format!("notchling: {other}"), 2),
    })
}

/// `notchling secrets [words]`.
pub fn secrets(args: &[String]) -> i32 {
    let (query, socket) = parse_find(args);
    let reply = ask(
        socket.as_deref(),
        ops::VAULT_FIND,
        json!({"query": query}),
        notchling_local_api::CALL_TIMEOUT,
        2,
    );
    match reply {
        Ok(reply) => {
            let (code, text) = find_report(&reply, &query, today());
            println!("{text}");
            code
        }
        Err((message, code)) => {
            eprintln!("{message}");
            code
        }
    }
}

/// `notchling secret <a,b> -- <command…>`.
pub fn secret(args: &[String]) -> i32 {
    let parsed = match parse_use(args) {
        Ok(p) => p,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let folder = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let args = json!({"names": parsed.names, "command": shown(&parsed.command), "folder": folder});
    let reply = match ask(
        parsed.socket.as_deref(),
        ops::VAULT_USE,
        args,
        USE_TIMEOUT,
        3,
    ) {
        Ok(reply) => reply,
        Err((message, code)) => {
            eprintln!("{message}");
            return code;
        }
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

/// `notchling secret-request <name> [...]`.
pub fn secret_request(args: &[String]) -> i32 {
    let (request, socket) = match parse_request(args) {
        Ok(p) => p,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let args = serde_json::to_value(&request).unwrap_or(Value::Null);
    match ask(
        socket.as_deref(),
        ops::VAULT_REQUEST,
        args,
        REQUEST_TIMEOUT,
        1,
    ) {
        Ok(reply) => match reply.get("entry").cloned().map(serde_json::from_value) {
            Some(Ok(entry)) => {
                let mut out = std::io::stdout();
                let _ = writeln!(out, "{}", saved_report(&entry, today()));
                0
            }
            _ => {
                eprintln!("Notchling didn't answer.");
                2
            }
        },
        Err((message, code)) => {
            eprintln!("{message}");
            code
        }
    }
}

#[cfg(test)]
#[path = "vault_cli_tests.rs"]
mod tests;
