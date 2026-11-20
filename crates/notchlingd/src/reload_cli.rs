use crate::layout::Layout;
use notchling_local_api::{ops, Client, ClientError, ClientKind, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

pub const USAGE: &str = "Usage: notchling session reload <id|self> [--after-turn] [--json] [--socket <path>]";

pub const DONE: i32 = 0;
pub const REFUSED: i32 = 1;
pub const TROUBLE: i32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub id: String,
    pub after_turn: bool,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut words: Vec<&str> = Vec::new();
    let (mut after_turn, mut json, mut socket) = (false, false, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--after-turn" => after_turn = true,
            "--json" => json = true,
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
        ["reload", id] if !id.is_empty() => Ok(Args {
            id: (*id).to_string(),
            after_turn,
            json,
            socket,
        }),
        _ => Err(USAGE.to_string()),
    }
}

fn connect(socket: Option<&std::path::Path>) -> Result<Client, ClientError> {
    let layout = Layout::from_env(socket).ok_or(ClientError::Unsupported)?;
    Client::connect(&layout.socket, ClientKind::Cli, env!("CARGO_PKG_VERSION"))
}

fn message_of(e: ClientError) -> (i32, String) {
    match e {
        ClientError::Api(api) => (REFUSED, api.message),
        ClientError::Timeout => (TROUBLE, "Notchling didn't answer.".to_string()),
        other => (TROUBLE, format!("notchling: {other}")),
    }
}

pub fn sentence(reply: &Value) -> String {
    let phase = reply
        .pointer("/task/phase")
        .and_then(Value::as_str)
        .unwrap_or("");
    match phase {
        "waiting" => match reply.pointer("/task/by/why").and_then(Value::as_str) {
            Some("tasks") => {
                let count = reply
                    .pointer("/task/by/count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                format!("Reload is waiting for {count} background tasks to finish.")
            }
            Some("dialog") => "Reload is waiting for the open question to be answered.".to_string(),
            _ => "Reload will start when the current turn ends.".to_string(),
        },
        "working" => "Reloading the session now.".to_string(),
        "done" => "The session was reloaded.".to_string(),
        "needs_you" => "Reload stopped; open the session in Notchling to see why.".to_string(),
        "cancelled" => "Reload was cancelled.".to_string(),
        _ => "Reload was not started.".to_string(),
    }
}

pub fn run_with(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return TROUBLE;
        }
    };
    let reply = connect(parsed.socket.as_deref()).and_then(|mut client| {
        client.set_timeout(CALL_TIMEOUT);
        client.call(
            ops::SESSION_RELOAD,
            json!({"id": parsed.id, "afterTurn": parsed.after_turn}),
        )
    });
    let reply = match reply {
        Ok(reply) => reply,
        Err(e) => {
            let (code, message) = message_of(e);
            let _ = writeln!(err, "{message}");
            return code;
        }
    };
    let shown = if parsed.json {
        serde_json::to_string_pretty(&reply).unwrap_or_else(|_| reply.to_string())
    } else {
        sentence(&reply)
    };
    let _ = writeln!(out, "{shown}");
    DONE
}

pub fn run(args: &[String]) -> i32 {
    run_with(args, &mut std::io::stdout(), &mut std::io::stderr())
}
