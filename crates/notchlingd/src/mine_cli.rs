use crate::contract::{self, Failure};
use notchling_local_api::ops;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    Outbox,
    Receipts,
    Activity,
    Prompts,
}

impl Read {
    pub fn usage(self) -> &'static str {
        match self {
            Read::Outbox => "Usage: notchling outbox [--json]",
            Read::Receipts => "Usage: notchling receipts [--json]",
            Read::Activity => {
                "Usage: notchling activity [--kind <event>]... [--since <ms>] [--limit <n>] [--json]"
            }
            Read::Prompts => "Usage: notchling prompts [--json]",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub kinds: Vec<String>,
    pub since: Option<u64>,
    pub limit: Option<u64>,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(read: Read, args: &[String]) -> Result<Args, String> {
    let usage = read.usage();
    let mut out = Args {
        kinds: Vec::new(),
        since: None,
        limit: None,
        json: false,
        socket: None,
    };
    let filters = read == Read::Activity;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |what: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{what} needs a value.\n{usage}"))
        };
        match arg.as_str() {
            "--json" => out.json = true,
            "--socket" => out.socket = Some(PathBuf::from(value("--socket")?)),
            "--kind" if filters => out.kinds.push(value("--kind")?),
            "--since" if filters => {
                out.since = Some(
                    value("--since")?
                        .parse()
                        .map_err(|_| format!("--since takes milliseconds.\n{usage}"))?,
                )
            }
            "--limit" if filters => {
                out.limit = Some(
                    value("--limit")?
                        .parse()
                        .map_err(|_| format!("--limit takes a number.\n{usage}"))?,
                )
            }
            "--help" | "-h" => return Err(usage.to_string()),
            other => return Err(format!("unknown argument {other}.\n{usage}")),
        }
    }
    Ok(out)
}

fn activity_args(args: &Args, kinds: &[&str]) -> Value {
    let mut out = Map::new();
    let mut names: Vec<Value> = args.kinds.iter().map(|k| json!(k)).collect();
    names.extend(kinds.iter().map(|k| json!(k)));
    if !names.is_empty() {
        out.insert("event_kinds".into(), Value::Array(names));
    }
    if let Some(since) = args.since {
        out.insert("since".into(), json!(since));
    }
    if let Some(limit) = args.limit {
        out.insert("limit".into(), json!(limit));
    }
    Value::Object(out)
}

fn text_of(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn lines(read: Read, reply: &Value) -> Vec<String> {
    let rows = reply["rows"].as_array().cloned().unwrap_or_default();
    if reply["reason"] == json!("no_session") {
        return vec!["This command reads the calling session's own data; none here.".to_owned()];
    }
    let mut out: Vec<String> = Vec::new();
    if rows.is_empty() {
        out.push("Nothing.".to_owned());
    }
    for row in &rows {
        out.push(match read {
            Read::Outbox => format!(
                "{}  {}  {}",
                text_of(row, "id"),
                text_of(row, "state"),
                text_of(row, "text").lines().next().unwrap_or_default()
            ),
            Read::Receipts => format!(
                "{}  {}  {}",
                text_of(row, "id"),
                text_of(row, "state"),
                text_of(row, "why")
            ),
            Read::Activity => format!(
                "{}  {}  {}",
                row["time_ms"],
                text_of(row, "event"),
                text_of(row, "result")
            ),
            Read::Prompts => format!(
                "{}  {}  {}",
                text_of(row, "id"),
                text_of(row, "kind"),
                text_of(row, "summary")
            ),
        });
    }
    if let Some(others) = reply.get("others") {
        out.push(format!(
            "Other sessions: {} messages in {} sessions.",
            others["messages"], others["sessions"]
        ));
    }
    out
}

fn say(text: &str) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{text}").and_then(|()| out.flush());
}

pub fn run(read: Read, args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let parsed = match parse(read, args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let mut client = match contract::connect(parsed.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    let reply = match read {
        Read::Outbox => contract::call(&mut client, ops::OUTBOX_MINE, json!({})),
        Read::Prompts => contract::call(&mut client, ops::PROMPTS_MINE, json!({})),
        Read::Activity => {
            contract::call(&mut client, ops::ACTIVITY_MINE, activity_args(&parsed, &[]))
        }
        Read::Receipts => contract::call(&mut client, ops::OUTBOX_MINE, json!({"receipts": true}))
            .and_then(|mut queued| {
                let events = contract::call(
                    &mut client,
                    ops::ACTIVITY_MINE,
                    activity_args(&parsed, &["session.outbox"]),
                )?;
                queued["events"] = events["rows"].clone();
                Ok(queued)
            }),
    };
    let reply = match reply {
        Ok(r) => r,
        Err(failure) => return failure.report(json),
    };
    if json {
        say(&contract::pretty(&reply));
    } else {
        for line in lines(read, &reply) {
            say(&line);
        }
    }
    contract::OK
}
