use crate::cli;
use crate::commands::help_value;
use crate::contract;
use notchling_local_api::{ops, CALL_TIMEOUT};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

pub fn help(json: bool, out: &mut dyn Write) -> i32 {
    let text = if json {
        contract::pretty(&help_value(env!("CARGO_PKG_VERSION")))
    } else {
        cli::usage().to_string()
    };
    let _ = writeln!(out, "{text}");
    contract::OK
}

fn names(reply: &Value, now: &str) -> Vec<String> {
    reply
        .get("ops")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|o| o.get("now").and_then(Value::as_str) == Some(now))
        .filter_map(|o| o.get("op").and_then(Value::as_str).map(str::to_string))
        .collect()
}

pub fn lines(reply: &Value) -> Vec<String> {
    let text = |path: &str| {
        reply
            .pointer(path)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let on: Vec<String> = reply
        .get("switches")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, on)| format!("{name} {}", if on == &json!(true) { "on" } else { "off" }))
        .collect();
    let attached = reply.pointer("/app/attached") == Some(&json!(true));
    let mut out = vec![
        format!(
            "Caller: {} ({}), session {}",
            text("/caller/tier"),
            text("/caller/tool"),
            reply
                .pointer("/caller/session")
                .and_then(Value::as_str)
                .unwrap_or("none")
        ),
        format!(
            "App: {}",
            if attached { "attached" } else { "not attached" }
        ),
        format!("Switches: {}", on.join(", ")),
    ];
    for (now, title) in [
        ("open", "Open now"),
        ("needs_consent", "Needs the user's yes"),
        ("needs_switch", "Needs a switch"),
        ("not_yet", "No command yet"),
        ("never", "Only the user"),
    ] {
        let list = names(reply, now);
        if !list.is_empty() {
            out.push(format!("{title}: {}", list.join(", ")));
        }
    }
    out
}

pub fn capabilities(json: bool, socket: Option<&Path>) -> i32 {
    let reply = contract::connect(socket).and_then(|mut client| {
        client.set_timeout(CALL_TIMEOUT);
        contract::call(&mut client, ops::DAEMON_CAPABILITIES, json!({}))
    });
    let reply = match reply {
        Ok(reply) => reply,
        Err(failure) => return failure.report(json),
    };
    let text = if json {
        contract::pretty(&reply)
    } else {
        lines(&reply).join("\n")
    };
    let _ = writeln!(std::io::stdout(), "{text}");
    contract::OK
}
