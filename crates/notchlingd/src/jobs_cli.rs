use crate::contract::{self, Failure};
use crate::settings_cli::Flags;
use notchling_engine::cost_profile::probe;
use notchling_local_api::ops;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const CALL_WAIT: Duration = Duration::from_secs(15);
const POLL_MS: u64 = 25_000;
const JOB_POLL: Duration = Duration::from_secs(2);
const TERMINAL: [&str; 4] = ["done", "failed", "interrupted", "lost"];

pub const DISPATCH_USAGE: &str = "Usage: notchling dispatch <machine> --tool <claude|codex|agy|shell> [--folder <path>] [--model <name>] [--effort <level>] [--text <prompt>] [--command <line>] [--secret <name>]... [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]";
pub const JOBS_USAGE: &str = "Usage: notchling jobs [--json] [--socket <path>]";
pub const JOB_USAGE: &str = "Usage: notchling job <id> [--wait] [--timeout <seconds>] [--json] [--socket <path>]\n       notchling job tail|return|stop <id> [--from <offset>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]";

#[derive(Debug, PartialEq, Eq)]
pub struct Scanned {
    pub words: Vec<String>,
    pub values: Vec<(String, String)>,
    pub json: bool,
    pub socket: Option<PathBuf>,
    pub flags: Flags,
}

impl Scanned {
    pub fn one(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn all(&self, name: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

pub fn scan(args: &[String], usage: &str, own: &[&str]) -> Result<Scanned, String> {
    let mut out = Scanned {
        words: Vec::new(),
        values: Vec::new(),
        json: false,
        socket: None,
        flags: Flags::default(),
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |flag: &str| match rest.next() {
            Some(v) if !v.is_empty() => Ok(v.clone()),
            _ => Err(format!("{flag} needs a value.\n{usage}")),
        };
        let seconds = |text: String, flag: &str| {
            text.parse::<u64>()
                .map_err(|_| format!("{flag} takes seconds.\n{usage}"))
        };
        match arg.as_str() {
            "--reason" => out.flags.reason = Some(value("--reason")?),
            "--key" => out.flags.key = Some(value("--key")?),
            "--wait" => out.flags.wait = true,
            "--no-wait" => out.flags.wait = false,
            "--dry-run" => out.flags.dry_run = true,
            "--timeout" => {
                out.flags.timeout = Duration::from_secs(seconds(value("--timeout")?, "--timeout")?)
            }
            "--expires-in" => {
                out.flags.expires_in = Some(seconds(value("--expires-in")?, "--expires-in")?)
            }
            "--json" => out.json = true,
            "--socket" => out.socket = Some(PathBuf::from(value("--socket")?)),
            "--help" | "-h" => return Err(usage.to_string()),
            other if own.contains(&other) => {
                let name = other.trim_start_matches("--").to_string();
                out.values.push((name, value(other)?));
            }
            other if other.starts_with("--") => {
                return Err(format!("Unknown option {other}.\n{usage}"))
            }
            _ => out.words.push(arg.clone()),
        }
    }
    Ok(out)
}

pub fn consent_of(scanned: &Scanned) -> Value {
    let mut consent = Map::new();
    if let Some(reason) = &scanned.flags.reason {
        consent.insert("reason".into(), json!(reason));
    }
    if let Some(seconds) = scanned.flags.expires_in {
        consent.insert("expiresInMs".into(), json!(seconds.saturating_mul(1000)));
    }
    if let Some(key) = &scanned.flags.key {
        consent.insert("key".into(), json!(key));
    }
    Value::Object(consent)
}

pub fn dispatch_body(scanned: &Scanned, folder: &str, fresh: &str) -> Result<Value, String> {
    let machine = match scanned.words.as_slice() {
        [machine] => machine.clone(),
        _ => return Err(DISPATCH_USAGE.to_string()),
    };
    let tool = scanned
        .one("tool")
        .ok_or_else(|| format!("--tool is needed.\n{DISPATCH_USAGE}"))?;
    let mut body = json!({
        "machine": machine, "tool": tool,
        "cwd": scanned.one("folder").unwrap_or(folder),
        "key": scanned.flags.key.clone().unwrap_or_else(|| fresh.to_string()),
        "consent": consent_of(scanned),
    });
    for name in ["model", "effort", "text", "command"] {
        if let Some(v) = scanned.one(name) {
            body[name] = json!(v);
        }
    }
    let secrets = scanned.all("secret");
    if !secrets.is_empty() {
        body["secrets"] = json!(secrets);
    }
    if scanned.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    Ok(body)
}

fn fresh_key() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("cli-{now:x}-{}", std::process::id())
}

pub fn tail_text(reply: &Value) -> String {
    let text = reply
        .pointer("/result/text")
        .or_else(|| reply.pointer("/request/result/text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    text.strip_suffix('\n').unwrap_or(text).to_string()
}

fn state_of(request: &Value) -> &str {
    request
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("failed")
}

pub(crate) fn first_call_wait(op: &str) -> Duration {
    if op == ops::COST_REQUEST {
        CALL_WAIT + probe::PLAN_WAIT
    } else {
        CALL_WAIT
    }
}

pub(crate) fn through_card(
    op: &str,
    body: Value,
    scanned: &Scanned,
    usage: &str,
    done: impl Fn(&Value, bool) -> String,
) -> i32 {
    let json = scanned.json;
    let end = match contract::deadline(scanned.flags.timeout, usage) {
        Ok(end) => end,
        Err(failure) => return failure.report(json),
    };
    let socket = scanned.socket.as_deref();
    let mut client = match contract::connect(socket) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(first_call_wait(op));
    let first = match contract::call(&mut client, op, body) {
        Ok(reply) => reply,
        Err(failure) => return failure.report(json),
    };
    if first.get("free").and_then(Value::as_bool) == Some(true) {
        if json {
            println!("{}", contract::pretty(&first));
        } else {
            println!("{}", done(&first, true));
        }
        return contract::OK;
    }
    if scanned.flags.dry_run {
        println!("{}", contract::pretty(&first));
        return contract::OK;
    }
    let request = first.get("request").cloned().unwrap_or(Value::Null);
    let id = request
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let ask = |left: Duration| {
        let slice = contract::wait_slice(left, POLL_MS);
        (
            json!({"id": id, "waitMs": slice}),
            CALL_WAIT + Duration::from_millis(slice),
        )
    };
    let request = match contract::wait_while_pending(request, scanned.flags.wait, end, || {
        contract::call_through_restart(&mut client, socket, ops::CONSENT_GET, &ask, end)
    }) {
        Ok(request) => request,
        Err(failure) => return failure.report(json),
    };
    if state_of(&request) != "applied" {
        return crate::consent_cli::failure_of(&request, op).report(json);
    }
    if json {
        println!("{}", contract::pretty(&json!({"request": request})));
    } else {
        println!("{}", done(&request, false));
    }
    contract::OK
}

fn usage_failure(message: String, json: bool) -> i32 {
    Failure::usage(message).report(json)
}

pub fn dispatch(raw: &[String]) -> i32 {
    let json = contract::wants_json(raw);
    let scanned = match scan(
        raw,
        DISPATCH_USAGE,
        &[
            "--tool",
            "--folder",
            "--model",
            "--effort",
            "--text",
            "--command",
            "--secret",
        ],
    ) {
        Ok(s) => s,
        Err(m) if m == DISPATCH_USAGE && raw.iter().any(|a| a == "--help" || a == "-h") => {
            println!("{m}");
            return contract::OK;
        }
        Err(m) => return usage_failure(m, json),
    };
    let folder = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let body = match dispatch_body(&scanned, &folder, &fresh_key()) {
        Ok(b) => b,
        Err(m) => return usage_failure(m, json),
    };
    through_card(
        ops::CLUSTER_DISPATCH,
        body,
        &scanned,
        DISPATCH_USAGE,
        |reply, _| {
            let job = reply
                .pointer("/request/result/job")
                .or_else(|| reply.get("job"))
                .and_then(Value::as_str)
                .unwrap_or("");
            format!("Sent the job {job} (the user said yes).")
        },
    )
}

pub fn jobs(raw: &[String]) -> i32 {
    let json = contract::wants_json(raw);
    let scanned = match scan(raw, JOBS_USAGE, &[]) {
        Ok(s) if s.words.is_empty() => s,
        Ok(_) => return usage_failure(JOBS_USAGE.to_string(), json),
        Err(m) => return usage_failure(m, json),
    };
    let mut client = match contract::connect(scanned.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(CALL_WAIT);
    match contract::call(&mut client, ops::JOBS_MINE, json!({})) {
        Ok(reply) if json => {
            println!("{}", contract::pretty(&reply));
            contract::OK
        }
        Ok(reply) => {
            for job in reply["jobs"].as_array().into_iter().flatten() {
                println!("{}", row_line(job));
            }
            contract::OK
        }
        Err(failure) => failure.report(json),
    }
}

pub fn row_line(job: &Value) -> String {
    let text = |key: &str| job.get(key).and_then(Value::as_str).unwrap_or("-");
    if job["own"] == json!(true) {
        format!(
            "{}  {}  {}  {}  {}",
            text("id"),
            text("state"),
            text("machine"),
            text("tool"),
            text("folder")
        )
    } else {
        format!("{}  {}", text("id"), text("state"))
    }
}

pub fn job(raw: &[String]) -> i32 {
    let json = contract::wants_json(raw);
    let verb = raw
        .first()
        .map(String::as_str)
        .filter(|w| ["tail", "return", "stop"].contains(w));
    let rest: &[String] = if verb.is_some() { &raw[1..] } else { raw };
    let scanned = match scan(rest, JOB_USAGE, &["--from"]) {
        Ok(s) => s,
        Err(m) if m == JOB_USAGE && raw.iter().any(|a| a == "--help" || a == "-h") => {
            println!("{m}");
            return contract::OK;
        }
        Err(m) => return usage_failure(m, json),
    };
    let [id] = scanned.words.as_slice() else {
        return usage_failure(JOB_USAGE.to_string(), json);
    };
    let Some(verb) = verb else {
        return follow(id, &scanned);
    };
    let (op, mut body) = match verb {
        "tail" => (ops::JOBS_TAIL, json!({"id": id})),
        "return" => (ops::JOBS_RETURN, json!({"id": id})),
        _ => (ops::JOBS_STOP, json!({"id": id})),
    };
    if let Some(from) = scanned.one("from") {
        match from.parse::<u64>() {
            Ok(n) if verb == "tail" => body["from"] = json!(n),
            _ => {
                return usage_failure(
                    format!("--from takes a number after job tail.\n{JOB_USAGE}"),
                    json,
                )
            }
        }
    }
    body["consent"] = consent_of(&scanned);
    if scanned.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    let word = match verb {
        "tail" => "Read the output of",
        "return" => "Brought back the result of",
        _ => "Stopped",
    };
    let id = id.clone();
    through_card(op, body, &scanned, JOB_USAGE, move |reply, free| {
        if verb == "tail" {
            return tail_text(reply);
        }
        let tail = if free { "" } else { " (the user said yes)" };
        format!("{word} the job {id}{tail}.")
    })
}

fn follow(id: &str, scanned: &Scanned) -> i32 {
    let json = scanned.json;
    let end = match contract::deadline(scanned.flags.timeout, JOB_USAGE) {
        Ok(end) => end,
        Err(failure) => return failure.report(json),
    };
    let mut client = match contract::connect(scanned.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(CALL_WAIT);
    loop {
        let reply = match contract::call(&mut client, ops::JOBS_GET, json!({"id": id})) {
            Ok(r) => r,
            Err(failure) => return failure.report(json),
        };
        let state = reply["state"].as_str().unwrap_or("");
        let ended = TERMINAL.contains(&state);
        if !scanned.flags.wait || ended {
            if json {
                println!("{}", contract::pretty(&reply));
            } else {
                println!("{}", row_line(&reply));
            }
            return contract::OK;
        }
        if Instant::now() >= end {
            return Failure::new(contract::EXPIRED, "timeout", "The job is still running.")
                .report_with(json, &reply);
        }
        std::thread::sleep(JOB_POLL);
    }
}

pub struct Recorded(Option<(notchling_local_api::Client, String)>);

pub fn delegate_tool_and_folder(args: &[String], cwd: &str) -> (String, String) {
    let tool = args
        .iter()
        .take_while(|a| a.as_str() != "--")
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_default();
    let folder = args
        .iter()
        .position(|a| a == "--folder")
        .and_then(|i| args.get(i + 1))
        .filter(|f| f.starts_with('/'))
        .cloned()
        .unwrap_or_else(|| cwd.to_string());
    (tool, folder)
}

pub fn delegate_begin(args: &[String], cwd: &str) -> Recorded {
    let (tool, folder) = delegate_tool_and_folder(args, cwd);
    if tool.is_empty() {
        return Recorded(None);
    }
    let Ok(mut client) = contract::connect(None) else {
        return Recorded(None);
    };
    client.set_timeout(Duration::from_secs(2));
    let reply = client.call(ops::DELEGATE_BEGIN, json!({"tool": tool, "folder": folder}));
    match reply
        .ok()
        .and_then(|r| r["id"].as_str().map(str::to_string))
    {
        Some(id) => Recorded(Some((client, id))),
        None => Recorded(None),
    }
}

pub fn delegate_end(recorded: Recorded, exit: i32) {
    if let Some((mut client, id)) = recorded.0 {
        let _ = client.call(ops::DELEGATE_END, json!({"id": id, "exit": exit}));
    }
}

#[cfg(test)]
#[path = "jobs_cli_tests.rs"]
mod tests;
