use crate::cost_report_ops::{COST_REPORT, MAX_DAYS};
use crate::cost_report_text;
use crate::layout::Layout;
use notchling_local_api::{Client, ClientError, ClientKind, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str =
    "Usage: notchling cost report [--days N] [--plan api|subscription|auto] [--json]";

pub const DONE: i32 = 0;
pub const REFUSED: i32 = 1;
pub const TROUBLE: i32 = 2;

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
            "This Notchling has no cost report yet. Ask the user to update it.".into()
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
    cost_report_text::render(reply)
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
    match client.call(COST_REPORT, request(&parsed)) {
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
