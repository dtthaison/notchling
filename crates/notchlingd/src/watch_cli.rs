use crate::contract::{self, Failure};
use notchling_local_api::{topics, ClientError, ErrorCode};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const USAGE: &str = "Usage: notchling watch [--topic <topic>]... [--until <state>] [--count <n>] [--timeout <seconds>] [--json]\nTopics: mine.consents, mine.asks, settings.changed, settings.switch";

const SLICE: Duration = Duration::from_millis(500);
const TOPICS: [&str; 4] = [
    topics::MINE_CONSENTS,
    topics::MINE_ASKS,
    "settings.changed",
    "settings.switch",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub topics: Vec<String>,
    pub explicit: bool,
    pub until: Option<String>,
    pub count: Option<u64>,
    pub timeout: Option<Duration>,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut found: Vec<String> = Vec::new();
    let (mut until, mut count, mut timeout, mut socket, mut json) =
        (None, None, None, None, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |what: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{what} needs a value.\n{USAGE}"))
        };
        match arg.as_str() {
            "--json" => json = true,
            "--topic" => {
                let topic = value("--topic")?;
                if !TOPICS.contains(&topic.as_str()) {
                    return Err(format!("No topic named {topic}.\n{USAGE}"));
                }
                if !found.contains(&topic) {
                    found.push(topic);
                }
            }
            "--until" => until = Some(value("--until")?).filter(|s| !s.is_empty()),
            "--count" => {
                let n: u64 = value("--count")?
                    .parse()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("--count takes a number above 0.\n{USAGE}"))?;
                count = Some(n);
            }
            "--timeout" => {
                let seconds: u64 = value("--timeout")?
                    .parse()
                    .map_err(|_| format!("--timeout takes seconds.\n{USAGE}"))?;
                timeout = Some(Duration::from_secs(seconds));
            }
            "--socket" => socket = Some(PathBuf::from(value("--socket")?)),
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other}.\n{USAGE}")),
        }
    }
    let explicit = !found.is_empty();
    let topics = if explicit {
        found
    } else {
        TOPICS.iter().map(|t| (*t).to_owned()).collect()
    };
    Ok(Args {
        topics,
        explicit,
        until,
        count,
        timeout,
        json,
        socket,
    })
}

fn wire_topics(wanted: &[String], public_only: bool) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for topic in wanted {
        let wire = match topic.as_str() {
            "settings.changed" | "settings.switch" => topics::SETTINGS,
            t if public_only && t.starts_with(topics::MINE_PREFIX) => continue,
            t => TOPICS.iter().find(|k| **k == t).copied().unwrap_or(topics::SETTINGS),
        };
        if !out.contains(&wire) {
            out.push(wire);
        }
    }
    out
}

pub fn wanted(topics_in: &[String], event: &str) -> bool {
    let topic = match event {
        "consent.changed" => topics::MINE_CONSENTS,
        "ask.changed" | "ask.removed" => topics::MINE_ASKS,
        "settings.changed" | "settings.switch" => event,
        _ => return false,
    };
    topics_in.iter().any(|t| t == topic)
}

fn reached(args: &Args, data: &Value) -> bool {
    match (&args.until, data.get("state").and_then(Value::as_str)) {
        (Some(until), Some(state)) => until == state,
        _ => false,
    }
}

fn subscribe(
    args: &Args,
    public_only: bool,
) -> Result<notchling_local_api::Client, (Failure, bool)> {
    let mut client = contract::connect(args.socket.as_deref()).map_err(|f| (f, false))?;
    let list = wire_topics(&args.topics, public_only);
    match client.subscribe(&list) {
        Ok(()) => Ok(client),
        Err(ClientError::Api(e))
            if e.code == ErrorCode::Forbidden && !args.explicit && !public_only =>
        {
            Err((Failure::from_api(None, e), true))
        }
        Err(e) => Err((Failure::from_client(Some("events.subscribe"), e), false)),
    }
}

fn open(args: &Args) -> Result<notchling_local_api::Client, Failure> {
    match subscribe(args, false) {
        Ok(client) => Ok(client),
        Err((_, true)) => subscribe(args, true).map_err(|(f, _)| f),
        Err((failure, false)) => Err(failure),
    }
}

pub fn run(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let mut client = match open(&parsed) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    let end = parsed.timeout.map(|t| Instant::now() + t);
    let mut seen: u64 = 0;
    let mut step = 0usize;
    let mut out = std::io::stdout();
    loop {
        let left = end.map(|e| e.saturating_duration_since(Instant::now()));
        if left.is_some_and(|l| l.is_zero()) {
            return Failure::new(contract::EXPIRED, "expired", "Nothing more came in time.")
                .with_op("events.subscribe")
                .report(json);
        }
        let slice = left.map_or(SLICE, |l| l.min(SLICE));
        match client.next_event(slice) {
            Ok(Some(event)) => {
                step = 0;
                if !wanted(&parsed.topics, &event.event) {
                    continue;
                }
                let line = serde_json::to_string(&event).unwrap_or_default();
                if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                    return contract::OK;
                }
                seen += 1;
                if reached(&parsed, &event.data) || parsed.count.is_some_and(|n| seen >= n) {
                    return contract::OK;
                }
            }
            Ok(None) => {}
            Err(e) if contract::transient(&e) => loop {
                if end.is_some_and(|e| Instant::now() >= e) {
                    break;
                }
                std::thread::sleep(contract::pause(step).min(
                    end.map_or(Duration::MAX, |e| e.saturating_duration_since(Instant::now())),
                ));
                step += 1;
                if let Ok(fresh) = open(&parsed) {
                    client = fresh;
                    break;
                }
            },
            Err(e) => return Failure::from_client(Some("events.subscribe"), e).report(json),
        }
    }
}
