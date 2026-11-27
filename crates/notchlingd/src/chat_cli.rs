use crate::contract::{self, Failure};
use notchling_local_api::{ops, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

pub const USAGE: &str =
    "Usage: notchling chat open-file <path> [--session <id>] [--json] [--socket <path>]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub path: String,
    pub session: Option<String>,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut words: Vec<&str> = Vec::new();
    let (mut session, mut json, mut socket) = (None, false, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--session" => match rest.next() {
                Some(id) if !id.is_empty() => session = Some(id.clone()),
                _ => return Err(format!("--session needs a value.\n{USAGE}")),
            },
            "--socket" => match rest.next() {
                Some(path) if !path.is_empty() => socket = Some(PathBuf::from(path)),
                _ => return Err(format!("--socket needs a value.\n{USAGE}")),
            },
            other if other.starts_with("--") => {
                return Err(format!("unknown option: {other}\n{USAGE}"))
            }
            other => words.push(other),
        }
    }
    match words.as_slice() {
        ["open-file", path] if !path.is_empty() => Ok(Args {
            path: absolute(path),
            session,
            json,
            socket,
        }),
        _ => Err(USAGE.to_string()),
    }
}

fn absolute(path: &str) -> String {
    let given = PathBuf::from(path);
    if given.is_absolute() {
        return path.to_string();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(given).to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

pub fn sentence(reply: &Value) -> String {
    match reply.get("path").and_then(Value::as_str) {
        Some(path) => format!("Opened {path} in Chat."),
        None => "Opened the file in Chat.".to_string(),
    }
}

pub fn run_with(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let json = contract::wants_json(args);
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(message) => return Failure::usage(message).emit(json, out, err),
    };
    let reply = contract::connect(parsed.socket.as_deref()).and_then(|mut client| {
        client.set_timeout(CALL_TIMEOUT);
        contract::call(
            &mut client,
            ops::CHAT_OPEN_FILE,
            json!({"path": parsed.path, "session": parsed.session}),
        )
    });
    let reply = match reply {
        Ok(reply) => reply,
        Err(failure) => return failure.emit(json, out, err),
    };
    let shown = if parsed.json {
        contract::pretty(&reply)
    } else {
        sentence(&reply)
    };
    let _ = writeln!(out, "{shown}");
    contract::OK
}

pub fn run(args: &[String]) -> i32 {
    run_with(args, &mut std::io::stdout(), &mut std::io::stderr())
}
