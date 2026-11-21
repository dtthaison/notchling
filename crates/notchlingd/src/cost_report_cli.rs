use crate::cost_report_ops::{COST_REPORT, MAX_DAYS};
use crate::cost_report_text;
use crate::contract::{self, Failure};
use std::io::Write;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str =
    "Usage: notchling cost report [--days N] [--plan api|subscription|auto] [--json]";

pub const DONE: i32 = contract::OK;

pub const WAIT: Duration = Duration::from_secs(150);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub days: Option<u32>,
    pub plan: Option<String>,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        days: None,
        plan: None,
        json: false,
        socket: None,
    };
    let mut rest = args.iter();
    match rest.next().map(String::as_str) {
        Some("report") => {}
        _ => return Err(USAGE.to_string()),
    }
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--days" => match rest.next().and_then(|v| v.parse::<u32>().ok()) {
                Some(n) if (1..=MAX_DAYS).contains(&n) => parsed.days = Some(n),
                _ => {
                    return Err(format!(
                        "--days needs a number from 1 to {MAX_DAYS}.\n{USAGE}"
                    ))
                }
            },
            "--plan" => match rest.next() {
                Some(v) if matches!(v.as_str(), "api" | "subscription" | "auto") => {
                    parsed.plan = Some(v.clone())
                }
                _ => return Err(format!("--plan is api, subscription or auto.\n{USAGE}")),
            },
            "--socket" => match rest.next() {
                Some(path) if !path.is_empty() => parsed.socket = Some(PathBuf::from(path)),
                _ => return Err(format!("--socket needs a value.\n{USAGE}")),
            },
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument: {other}.\n{USAGE}")),
        }
    }
    Ok(parsed)
}

pub fn request(args: &Args) -> Value {
    let mut body = json!({});
    if let Some(days) = args.days {
        body["days"] = json!(days);
    }
    if let Some(plan) = &args.plan {
        body["plan"] = json!(plan);
    }
    body
}

pub fn shown(reply: &Value, json: bool) -> String {
    if json {
        return serde_json::to_string_pretty(reply).unwrap_or_else(|_| reply.to_string());
    }
    cost_report_text::render(reply)
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
    match contract::call(&mut client, COST_REPORT, request(&parsed)) {
        Ok(reply) => {
            let _ = writeln!(std::io::stdout(), "{}", shown(&reply, parsed.json));
            DONE
        }
        Err(failure) => failure.report(json),
    }
}
