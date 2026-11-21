use crate::contract::{self, Failure};
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str = "Usage: notchling models [check] [--json]\n       notchling models update [--json] [--no-skills] [--force]";

pub const DONE: i32 = contract::OK;

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
    let json = contract::wants_json(args);
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let mut client = match contract::connect(parsed.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(WAIT);
    let (op, args) = request(&parsed);
    match contract::call(&mut client, op, args) {
        Ok(reply) => {
            println!("{}", shown(&reply, parsed.json));
            DONE
        }
        Err(failure) => failure.report(json),
    }
}

#[cfg(test)]
#[path = "models_cli_tests.rs"]
mod tests;
