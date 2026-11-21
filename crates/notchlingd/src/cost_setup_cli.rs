use crate::contract::{self, Failure};
use notchling_local_api::{ops, ClientError};
use std::io::Write;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str = "Usage: notchling cost setup [--apply] [--consent <a,b>] [--wait] [--json]\n       notchling cost check [--json]\n       notchling cost undo <claude|codex|agy> [--json]";

pub const DONE: i32 = contract::OK;
pub const WAIT: Duration = Duration::from_secs(150);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    Setup { apply: bool },
    Check,
    Undo(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub job: Job,
    pub json: bool,
    pub consent: Vec<String>,
    pub wait: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut words: Vec<&str> = Vec::new();
    let mut apply = false;
    let mut json = false;
    let mut wait = false;
    let mut consent: Vec<String> = Vec::new();
    let mut socket = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--apply" => apply = true,
            "--json" => json = true,
            "--wait" => wait = true,
            "--consent" => match rest.next() {
                Some(list) if !list.is_empty() => {
                    consent.extend(list.split(',').map(|c| c.trim().to_string()))
                }
                _ => return Err(format!("--consent needs a list.\n{USAGE}")),
            },
            "--socket" => match rest.next() {
                Some(path) if !path.is_empty() => socket = Some(PathBuf::from(path)),
                _ => return Err(format!("--socket needs a value.\n{USAGE}")),
            },
            "--help" | "-h" => return Err(USAGE.to_string()),
            word if !word.starts_with("--") => words.push(word),
            other => return Err(format!("unknown argument: {other}.\n{USAGE}")),
        }
    }
    let job = match words.as_slice() {
        ["setup"] => Job::Setup { apply },
        ["check"] => Job::Check,
        ["undo", tool @ ("claude" | "codex" | "agy" | "antigravity")] => {
            Job::Undo(tool.to_string())
        }
        _ => return Err(USAGE.to_string()),
    };
    if apply && !matches!(job, Job::Setup { .. }) {
        return Err(format!("--apply goes with setup.\n{USAGE}"));
    }
    Ok(Args {
        job,
        json,
        consent,
        wait,
        socket,
    })
}

pub fn request(args: &Args) -> (&'static str, Value) {
    let mut body = json!({});
    if !args.consent.is_empty() {
        body["consent"] = json!(args.consent);
    }
    if args.wait {
        body["wait"] = json!(true);
    }
    match &args.job {
        Job::Setup { apply: false } => (ops::COST_SETUP_PLAN, body),
        Job::Setup { apply: true } => (ops::COST_SETUP_APPLY, body),
        Job::Check => (ops::COST_SETUP_CHECK, json!({})),
        Job::Undo(tool) => (ops::COST_SETUP_UNDO, json!({"tool": tool})),
    }
}

pub fn failure_of(e: ClientError, op: &str, applying: bool) -> Failure {
    let failure = Failure::from_client(Some(op), e);
    if applying && failure.exit == contract::REFUSED {
        return failure.with_human("Only the Notchling app writes these files. Ask the user to open Settings, Cost Profile, and press Set Up.");
    }
    failure
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn plan_text(plan: &Value, out: &mut String) {
    let actions = plan["actions"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    for a in actions {
        let kind = text(a, "kind");
        if kind == "unchanged" {
            continue;
        }
        out.push_str(&format!(
            "{kind:<9} {}  {}",
            text(a, "tool"),
            text(a, "path")
        ));
        if let Some(reason) = a["reason"].as_str() {
            out.push_str(&format!("  ({reason})"));
        }
        if let Some(need) = a["needs"].as_str().filter(|_| kind == "offer") {
            out.push_str(&format!("  needs --consent {need}"));
        }
        out.push('\n');
        let diff = text(a, "diff");
        if !diff.is_empty() {
            out.push_str(diff);
            if !diff.ends_with('\n') {
                out.push('\n');
            }
        }
        for key in a["keys"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "    {}: {} -> {}\n",
                text(key, "key"),
                key["before"].as_str().unwrap_or("(unset)"),
                text(key, "after")
            ));
        }
    }
    if out.is_empty() {
        out.push_str("Nothing to do.\n");
    }
    for line in plan["report"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "line {:>3}  {:<12} {}\n",
            line["line"].as_u64().unwrap_or(0),
            text(line, "fate"),
            text(line, "text")
        ));
    }
}

pub fn shown(args: &Args, reply: &Value) -> String {
    if args.json {
        return serde_json::to_string_pretty(reply).unwrap_or_else(|_| reply.to_string());
    }
    let mut out = String::new();
    match &args.job {
        Job::Setup { apply } => {
            plan_text(&reply["plan"], &mut out);
            if *apply {
                if reply["on"] == json!(false) {
                    out.push_str(&format!("Not applied: {}\n", text(reply, "message")));
                }
                for r in reply["results"].as_array().into_iter().flatten() {
                    if r["done"] == json!(true) || r["error"].is_string() {
                        out.push_str(&format!(
                            "{}  {}  {}\n",
                            if r["done"] == json!(true) {
                                "done"
                            } else {
                                "failed"
                            },
                            text(r, "kind"),
                            text(r, "path")
                        ));
                    }
                }
            } else {
                out.push_str("This was a dry run. Nothing was written.\n");
            }
        }
        Job::Check => {
            for r in reply["rows"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{:<16} {}  {}\n",
                    text(r, "state"),
                    text(r, "tool"),
                    text(r, "path")
                ));
            }
            if out.is_empty() {
                out.push_str("Nothing recorded yet.\n");
            }
        }
        Job::Undo(_) => {
            if reply["on"] == json!(false) {
                out.push_str(&format!("Not undone: {}\n", text(reply, "message")));
            }
            for r in reply["results"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{:<9} {}{}\n",
                    text(r, "outcome"),
                    text(r, "path"),
                    r["detail"]
                        .as_str()
                        .map(|d| format!("  ({d})"))
                        .unwrap_or_default()
                ));
            }
        }
    }
    out.trim_end().to_string()
}

pub fn run(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let writes = !matches!(parsed.job, Job::Setup { apply: false } | Job::Check);
    let mut client = match contract::connect(parsed.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(WAIT);
    let (op, body) = request(&parsed);
    match client.call(op, body) {
        Ok(reply) => {
            let _ = writeln!(std::io::stdout(), "{}", shown(&parsed, &reply));
            DONE
        }
        Err(e) => failure_of(e, op, writes).report(json),
    }
}

#[cfg(test)]
#[path = "cost_setup_cli_tests.rs"]
mod tests;
