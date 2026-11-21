use crate::contract::{self, Failure};
use notchling_local_api::{ops, CALL_TIMEOUT};
use serde_json::{json, Value};
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
                Some(p) if !p.is_empty() => out.socket = Some(PathBuf::from(p)),
                _ => return Err("--socket needs a value".into()),
            },
            flag if takes_value(name, flag) => match rest.next() {
                Some(v) if !v.is_empty() => out.flags.push((flag.to_string(), Some(v.clone()))),
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
    println!(
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

pub fn run(name: &str, args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let p = match parse(name, args) {
        Ok(p) => p,
        Err(message) => return Failure::usage(message).report(json),
    };
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
        "limits" => return limits(&p, json, &mut ask),
        "search" => {
            let mut query = json!({"text": p.words.join(" ")});
            if let Some(kind) = flag(&p, "--kind") {
                if !matches!(kind, "pages" | "sessions") {
                    return Failure::usage("--kind is pages or sessions").report(json);
                }
                query["kind"] = json!(kind);
            }
            if let Some(since) = flag(&p, "--since") {
                match since.parse::<f64>() {
                    Ok(s) => query["since"] = json!(s),
                    Err(_) => return Failure::usage("--since takes seconds").report(json),
                }
            }
            ask(ops::SEARCH_QUERY, query)
        }
        "switches" => switches(&mut ask),
        "awake" => return awake(&p, json, &mut ask, args),
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

fn awake(p: &Parsed, json: bool, ask: &mut Ask<'_>, args: &[String]) -> i32 {
    match p.words.as_slice() {
        [] => match ask(ops::AWAKE_STATE, json!({})) {
            Ok(value) => {
                show(&value);
                contract::OK
            }
            Err(failure) => failure.report(json),
        },
        [word] if word == "on" || word == "off" => {
            let mut set = vec!["set".to_string(), "mode".to_string(), word.clone()];
            set.extend(args.iter().filter(|a| a.as_str() != word).cloned());
            crate::settings_cli::run(&set)
        }
        _ => Failure::usage("awake takes on or off").report(json),
    }
}

fn limits(p: &Parsed, json: bool, ask: &mut Ask<'_>) -> i32 {
    let mut query = json!({});
    if let Some(tool) = flag(p, "--tool") {
        let Some(tool) = tool_name(tool) else {
            return Failure::usage("--tool is claude, codex or agy").report(json);
        };
        query["tool"] = json!(tool);
    }
    if let Some(pool) = flag(p, "--pool") {
        if flag(p, "--tool").is_none() {
            return Failure::usage("--pool needs --tool").report(json);
        }
        query["pool"] = json!(pool);
    }
    let reply = match ask(ops::LIMITS_SUMMARY, query.clone()) {
        Ok(r) => r,
        Err(failure) => return failure.report(json),
    };
    if query.get("tool").is_none() {
        show(&reply);
        return contract::OK;
    }
    if reply["reason"] == json!("no_session") {
        return Failure::failed("No session: limits are read for a verified session only.")
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
    let why = verdict["reason"].as_str().unwrap_or("unknown");
    let tool = query["tool"].as_str().unwrap_or("");
    Failure::failed(format!("{tool} is not usable now: {why}."))
        .with_op(ops::LIMITS_SUMMARY)
        .report_with(json, &verdict)
}
