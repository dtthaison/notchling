use crate::contract::{self, Failure};
use notchling_local_api::{ops, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str = "Usage: notchling shells [--json] [--socket <path>]\n       notchling shells stop <id> [--json] [--socket <path>]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sub {
    List,
    Stop(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub sub: Sub,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut words: Vec<&str> = Vec::new();
    let (mut json, mut socket) = (false, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--socket" => match rest.next() {
                Some(path) if !path.is_empty() => socket = Some(PathBuf::from(path)),
                _ => return Err(format!("--socket needs a value.\n{USAGE}")),
            },
            "--help" | "-h" => return Err(USAGE.to_string()),
            other if other.starts_with("--") => {
                return Err(format!("unknown flag {other}.\n{USAGE}"))
            }
            other => words.push(other),
        }
    }
    let sub = match words.as_slice() {
        [] => Sub::List,
        ["stop", id] if !id.is_empty() => Sub::Stop((*id).to_string()),
        _ => return Err(USAGE.to_string()),
    };
    Ok(Args { sub, json, socket })
}

pub fn age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h{:02}m", secs / 3600, secs % 3600 / 60),
    }
}

pub fn line(shell: &Value) -> String {
    let text = |key: &str| shell.get(key).and_then(Value::as_str).unwrap_or("");
    let running = shell
        .get("runningSecs")
        .and_then(Value::as_u64)
        .map_or("-".to_string(), age);
    let stuck = if shell.get("stuck") == Some(&json!(true)) {
        "stuck"
    } else {
        "-"
    };
    let command: String = text("command").chars().take(100).collect();
    format!(
        "{}  {}  {}  {}  {}",
        text("id"),
        text("status"),
        running,
        stuck,
        command.replace('\n', " ")
    )
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    );
}

pub fn run(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let mut client = match contract::connect(parsed.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(CALL_TIMEOUT + Duration::from_secs(10));
    match &parsed.sub {
        Sub::List => {
            let reply = match contract::call(&mut client, ops::SHELLS_MINE, json!({})) {
                Ok(r) => r,
                Err(f) => return f.report(json),
            };
            if json {
                print_json(&reply);
            } else {
                let rows = reply["shells"].as_array().cloned().unwrap_or_default();
                if rows.is_empty() {
                    println!("No background shells in this session.");
                }
                for row in &rows {
                    println!("{}", line(row));
                }
            }
            contract::OK
        }
        Sub::Stop(id) => {
            let reply = match contract::call(&mut client, ops::SHELLS_STOP, json!({"id": id})) {
                Ok(r) => r,
                Err(f) => return f.report(json),
            };
            if json {
                print_json(&reply);
            } else if reply["stopped"] == json!(true) {
                println!("Stopped {id}.");
            } else {
                println!("{id} had already ended.");
            }
            contract::OK
        }
    }
}

#[cfg(test)]
#[path = "shells_cli_tests.rs"]
mod tests;
