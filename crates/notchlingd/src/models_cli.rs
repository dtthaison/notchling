use crate::layout::Layout;
use notchling_local_api::{ops, Client, ClientError, ClientKind, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str = "Usage: notchling models [check] [--json]\n       notchling models update [--json] [--no-skills] [--force]";

pub const DONE: i32 = 0;
pub const REFUSED: i32 = 1;
pub const TROUBLE: i32 = 2;

pub const WAIT: Duration = Duration::from_secs(150);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub update: bool,
    pub json: bool,
    pub skills: bool,
    pub force: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        update: false,
        json: false,
        skills: true,
        force: false,
        socket: None,
    };
    let mut words: Vec<&str> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--no-skills" => parsed.skills = false,
            "--force" => parsed.force = true,
            "--socket" => match rest.next() {
                Some(path) if !path.is_empty() => parsed.socket = Some(PathBuf::from(path)),
                _ => return Err(format!("--socket needs a value.\n{USAGE}")),
            },
            "--help" | "-h" => return Err(USAGE.to_string()),
            word if !word.starts_with("--") => words.push(word),
            other => return Err(format!("unknown argument: {other}.\n{USAGE}")),
        }
    }
    match words.as_slice() {
        [] | ["check"] => {}
        ["update"] => parsed.update = true,
        _ => return Err(USAGE.to_string()),
    }
    Ok(parsed)
}

pub fn request(args: &Args) -> (&'static str, Value) {
    let op = if args.update {
        ops::MODELS_UPDATE
    } else {
        ops::MODELS_CHECK
    };
    let mut body = json!({ "skills": args.skills });
    if args.force {
        body["force"] = json!(true);
    }
    (op, body)
}

pub fn code_of(e: &ClientError) -> i32 {
    match e {
        ClientError::Api(api) => match api.code {
            ErrorCode::BadRequest | ErrorCode::Forbidden | ErrorCode::Failed => REFUSED,
            _ => TROUBLE,
        },
        _ => TROUBLE,
    }
}

pub fn message_of(e: ClientError) -> String {
    match e {
        ClientError::NotRunning(_) | ClientError::Unsupported => {
            "Notchling isn't running. Ask the user to open it, then try again.".into()
        }
        ClientError::Api(api) if api.code == ErrorCode::UnknownOp => {
            "This Notchling has no models command yet. Ask the user to update it.".into()
        }
        ClientError::Api(api) => api.message,
        ClientError::Timeout => "Notchling didn't answer in time.".into(),
        other => format!("notchling: {other}"),
    }
}

pub fn shown(reply: &Value, json: bool) -> String {
    if json {
        return serde_json::to_string_pretty(reply).unwrap_or_else(|_| reply.to_string());
    }
    reply
        .get("text")
        .and_then(Value::as_str)
        .map_or_else(|| reply.to_string(), str::to_string)
}

pub fn run(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(message) => {
            eprintln!("{message}");
            return TROUBLE;
        }
    };
    let Some(layout) = Layout::from_env(parsed.socket.as_deref()) else {
        eprintln!("{}", message_of(ClientError::Unsupported));
        return TROUBLE;
    };
    let mut client =
        match Client::connect(&layout.socket, ClientKind::Cli, env!("CARGO_PKG_VERSION")) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}", message_of(e));
                return TROUBLE;
            }
        };
    client.set_timeout(WAIT);
    let (op, args) = request(&parsed);
    match client.call(op, args) {
        Ok(reply) => {
            println!("{}", shown(&reply, parsed.json));
            DONE
        }
        Err(e) => {
            let code = code_of(&e);
            eprintln!("{}", message_of(e));
            code
        }
    }
}

#[cfg(test)]
#[path = "models_cli_tests.rs"]
mod tests;
