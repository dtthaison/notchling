use crate::contract::{self, Failure};
use notchling_local_api::{ops, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

pub const NAMES: [&str; 9] = [
    "inbox", "accounts", "limits", "orgs", "search", "switches", "awake", "versions", "roles",
];

struct Parsed {
    socket: Option<PathBuf>,
    words: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

type Ask<'a> = dyn FnMut(&str, Value) -> Result<Value, Failure> + 'a;

fn takes_value(name: &str, flag: &str) -> bool {
    matches!(
        (name, flag),
        ("search", "--kind" | "--since") | ("limits", "--tool" | "--pool")
    )
}

fn is_value(v: &str) -> bool {
    !v.is_empty() && !v.starts_with("--")
}

fn parse(name: &str, args: &[String]) -> Result<Parsed, String> {
    let mut out = Parsed {
        socket: None,
        words: Vec::new(),
        flags: Vec::new(),
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => {}
            "--socket" => match rest.next() {
                Some(p) if is_value(p) => out.socket = Some(PathBuf::from(p)),
                _ => return Err("--socket needs a value".into()),
            },
            flag if takes_value(name, flag) => match rest.next() {
                Some(v) if is_value(v) => out.flags.push((flag.to_string(), Some(v.clone()))),
                _ => return Err(format!("{flag} needs a value")),
            },
            "--latest" | "--workers" if name == "versions" => out.flags.push((arg.clone(), None)),
            other if other.starts_with("--") => {
                return Err(format!("unknown argument for {name}: {other}"))
            }
            _ => out.words.push(arg.clone()),
        }
    }
    if !matches!(name, "search" | "awake") && !out.words.is_empty() {
        return Err(format!("{name} takes no words"));
    }
    Ok(out)
}

fn flag<'a>(p: &'a Parsed, name: &str) -> Option<&'a str> {
    p.flags
        .iter()
        .find(|(f, _)| f == name)
        .and_then(|(_, v)| v.as_deref())
}

fn has(p: &Parsed, name: &str) -> bool {
    p.flags.iter().any(|(f, _)| f == name)
}

fn show(value: &Value) {
    let _ = writeln!(
        std::io::stdout(),
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    );
}

fn tool_name(tool: &str) -> Option<&'static str> {
    match tool {
        "claude" => Some("claude"),
        "codex" => Some("codex"),
        "agy" | "antigravity" => Some("agy"),
        _ => None,
    }
}

fn check(name: &str, p: &Parsed) -> Result<Value, Failure> {
    match name {
        "limits" => {
            let mut query = json!({});
            if let Some(tool) = flag(p, "--tool") {
                let Some(tool) = tool_name(tool) else {
                    return Err(Failure::usage("--tool is claude, codex or agy"));
                };
                query["tool"] = json!(tool);
            }
            if let Some(pool) = flag(p, "--pool") {
                if flag(p, "--tool").is_none() {
                    return Err(Failure::usage("--pool needs --tool"));
                }
                query["pool"] = json!(pool);
            }
            Ok(query)
        }
        "search" => {
            let mut query = json!({"text": p.words.join(" ")});
            if let Some(kind) = flag(p, "--kind") {
                if !matches!(kind, "pages" | "sessions") {
                    return Err(Failure::usage("--kind is pages or sessions"));
                }
                query["kind"] = json!(kind);
            }
            if let Some(since) = flag(p, "--since") {
                match since.parse::<f64>() {
                    Ok(s) if s.is_finite() && s >= 0.0 => query["since"] = json!(s),
                    _ => return Err(Failure::usage("--since takes seconds")),
                }
            }
            Ok(query)
        }
        "awake" => match p.words.as_slice() {
            [] => Ok(json!({})),
            [word] if word == "on" || word == "off" => Ok(json!({"mode": word})),
            _ => Err(Failure::usage("awake takes on or off")),
        },
        _ => Ok(json!({})),
    }
}

fn awake_set(word: &str, p: &Parsed, json: bool) -> Vec<String> {
    let mut set = vec!["set".to_string(), "mode".to_string(), word.to_string()];
    if let Some(socket) = &p.socket {
        set.push("--socket".into());
        set.push(socket.display().to_string());
    }
    if json {
        set.push("--json".into());
    }
    set
}

pub fn run(name: &str, args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let p = match parse(name, args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
    let query = match check(name, &p) {
        Ok(q) => q,
        Err(failure) => return failure.report(json),
    };
    if let Some(word) = query["mode"].as_str() {
        return crate::settings_cli::run(&awake_set(word, &p, json));
    }
    let mut client = match contract::connect(p.socket.as_deref()) {
        Ok(c) => c,
        Err(failure) => return failure.report(json),
    };
    client.set_timeout(CALL_TIMEOUT);
    let mut ask = |op: &str, args: Value| contract::call(&mut client, op, args);
    let result = match name {
        "inbox" => ask(ops::INBOX_MINE, json!({})),
        "accounts" => ask(ops::ACCOUNTS_SUMMARY, json!({})),
        "orgs" => ask(ops::ORGANIZATIONS_MINE, json!({})),
        "roles" => ask(ops::CATALOG_ROLES, json!({})),
        "limits" => return limits(query, json, &mut ask),
        "search" => ask(ops::SEARCH_QUERY, query),
        "switches" => switches(&mut ask),
        "awake" => ask(ops::AWAKE_STATE, json!({})),
        "versions" => versions(&p, &mut ask),
        _ => return Failure::usage(format!("unknown command: {name}")).report(json),
    };
    match result {
        Ok(value) => {
            show(&value);
            contract::OK
        }
        Err(failure) => failure.report(json),
    }
}

fn switches(ask: &mut Ask<'_>) -> Result<Value, Failure> {
    let mut all = ask(ops::SETTINGS_SWITCHES, json!({}))?;
    if let Ok(caps) = ask(ops::DAEMON_CAPABILITIES, json!({})) {
        if let (Some(map), Some(more)) = (all.as_object_mut(), caps["switches"].as_object()) {
            for (k, v) in more {
                map.entry(k.clone()).or_insert(v.clone());
            }
        }
    }
    Ok(all)
}

fn versions(p: &Parsed, ask: &mut Ask<'_>) -> Result<Value, Failure> {
    let mut out = json!({"installed": ask(ops::VERSIONS_GET, json!({}))?});
    if has(p, "--latest") {
        out["latest"] = ask(ops::VERSIONS_LATEST, json!({}))?;
    }
    if has(p, "--workers") {
        out["workers"] = ask(ops::VERSIONS_WORKERS, json!({}))?;
    }
    Ok(out)
}

fn limits(query: Value, json: bool, ask: &mut Ask<'_>) -> i32 {
    let reply = match ask(ops::LIMITS_SUMMARY, query.clone()) {
        Ok(r) => r,
        Err(failure) => return failure.report(json),
    };
    if query.get("tool").is_none() {
        show(&reply);
        return contract::OK;
    }
    if reply["reason"] == json!("no_session") {
        return Failure::new(
            contract::FAILED,
            "no_session",
            "No session: limits are read for a verified session only.",
        )
        .with_op(ops::LIMITS_SUMMARY)
        .report_with(json, &reply);
    }
    let verdict = if query.get("pool").is_some() {
        reply["pool"].clone()
    } else {
        reply["tool"].clone()
    };
    if verdict["usable"] == json!(true) {
        show(&verdict);
        return contract::OK;
    }
    not_usable(&query, &verdict)
        .with_op(ops::LIMITS_SUMMARY)
        .report_with(json, &verdict)
}

fn names_of(verdict: &Value, key: &str) -> String {
    verdict[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn not_usable(query: &Value, verdict: &Value) -> Failure {
    let why = verdict["reason"].as_str().unwrap_or("unknown");
    let tool = query["tool"].as_str().unwrap_or("");
    let pool = query["pool"].as_str().unwrap_or("");
    let message = match why {
        "unknown_tool" => format!(
            "Unknown tool {tool}. Pick one of: {}.",
            names_of(verdict, "tools")
        ),
        "unknown_pool" => format!(
            "{tool} has no pool {pool}. Pick a pool with --pool: {}.",
            names_of(verdict, "pools")
        ),
        "ambiguous_pool" => format!(
            "More than one pool of {tool} matches {pool}. Pick a pool with --pool: {}.",
            names_of(verdict, "pools")
        ),
        _ => format!("{tool} is not usable now: {why}."),
    };
    match contract::exit_of_reason(why).filter(|_| contract::REASONS.contains(&why)) {
        Some(exit) => Failure::new(exit, why, message),
        None => Failure::failed(message),
    }
}

#[cfg(test)]
#[path = "reads_cli_tests.rs"]
mod tests;
