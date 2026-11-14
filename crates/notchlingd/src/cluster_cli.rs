use crate::layout::Layout;
use notchling_local_api::{ops, Client, ClientError, ClientKind, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub const USAGE: &str = "Usage: notchling cluster status [--json]\n       notchling cluster token\n       notchling cluster join --token-stdin [--json]";
pub const TOKEN_ON_STDIN: &str = "pass the token on stdin: notchling cluster join --token-stdin";

const TOKEN_MAX: usize = 4096;

pub const DONE: i32 = 0;
pub const REFUSED: i32 = 1;
pub const TROUBLE: i32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sub {
    Status,
    Token,
    Join,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub sub: Sub,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut words: Vec<&str> = Vec::new();
    let (mut json, mut stdin, mut socket) = (false, false, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--token-stdin" => stdin = true,
            "--socket" => match rest.next() {
                Some(path) if !path.is_empty() => socket = Some(PathBuf::from(path)),
                _ => return Err(format!("--socket needs a value.\n{USAGE}")),
            },
            "--token" => return Err(TOKEN_ON_STDIN.to_string()),
            other if other.starts_with("--token=") || other.starts_with("nlj1.") => {
                return Err(TOKEN_ON_STDIN.to_string())
            }
            other => words.push(other),
        }
    }
    let sub = match words.as_slice() {
        ["status"] => Sub::Status,
        ["token"] => Sub::Token,
        ["join"] if stdin => Sub::Join,
        ["join", ..] => return Err(TOKEN_ON_STDIN.to_string()),
        _ => return Err(USAGE.to_string()),
    };
    if stdin && sub != Sub::Join {
        return Err(USAGE.to_string());
    }
    Ok(Args { sub, json, socket })
}

pub fn read_token(input: &mut dyn BufRead) -> Result<String, String> {
    let mut line = String::new();
    let mut limited = std::io::Read::take(input, TOKEN_MAX as u64 + 1);
    limited
        .read_line(&mut line)
        .map_err(|e| format!("reading the token failed: {e}"))?;
    if line.len() > TOKEN_MAX {
        return Err("the token is too long".to_string());
    }
    let token = line.trim();
    if token.is_empty() {
        return Err("no token on stdin".to_string());
    }
    Ok(token.to_string())
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn status_lines(status: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    let cluster = &status["cluster"];
    if cluster.is_null() {
        lines.push("Not in a cluster.".to_string());
    } else {
        lines.push(format!(
            "Cluster {} ({}), term {}",
            text(cluster, "name"),
            text(cluster, "id"),
            cluster["term"]
        ));
    }
    lines.push(format!("This node: {}", text(status, "me")));
    if let Some(error) = status.get("error").and_then(Value::as_str) {
        lines.push(format!("Trouble: {error}"));
    }
    for m in status["members"].as_array().into_iter().flatten() {
        let online = if m["online"] == json!(true) {
            "online"
        } else {
            "offline"
        };
        lines.push(format!(
            "  {}  {}  {}  {}  {}",
            text(m, "name"),
            text(m, "role"),
            text(m, "state"),
            online,
            text(m, "id")
        ));
    }
    lines
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

pub fn run_with(
    args: &[String],
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return TROUBLE;
        }
    };
    let token = match parsed.sub {
        Sub::Join => match read_token(input) {
            Ok(token) => Some(token),
            Err(message) => {
                let _ = writeln!(err, "{message}");
                return TROUBLE;
            }
        },
        _ => None,
    };
    let reply = connect(parsed.socket.as_deref()).and_then(|mut client| {
        client.set_timeout(CALL_TIMEOUT + CALL_TIMEOUT);
        match (&parsed.sub, token) {
            (Sub::Status, _) => client.call(ops::CLUSTER_STATUS, json!({})),
            (Sub::Token, _) => client.call(ops::CLUSTER_JOIN_TOKEN, json!({})),
            (Sub::Join, token) => client.call(ops::CLUSTER_JOIN, json!({"token": token})),
        }
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
        match parsed.sub {
            Sub::Status => status_lines(&reply).join("\n"),
            Sub::Token => text(&reply, "token").to_string(),
            Sub::Join if text(&reply, "state") == "joined" => "Joined the cluster.".to_string(),
            Sub::Join => {
                "Asked to join; the cluster's primary must allow it in its Inbox.".to_string()
            }
        }
    };
    let _ = writeln!(out, "{shown}");
    DONE
}

pub fn run(args: &[String]) -> i32 {
    let stdin = std::io::stdin();
    run_with(
        args,
        &mut stdin.lock(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

#[cfg(test)]
#[path = "cluster_cli_tests.rs"]
mod tests;
