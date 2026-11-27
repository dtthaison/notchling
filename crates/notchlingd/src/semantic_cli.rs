use crate::contract::{self, Failure};
use crate::jobs_cli::{consent_of, scan, through_card, Scanned};
use notchling_local_api::ops;
use serde_json::{json, Value};

pub const USAGE: &str = "Usage: notchling semantic download <model> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]";

pub fn request(scanned: &Scanned) -> Result<(&'static str, Value), String> {
    let model = match scanned.words.as_slice() {
        [model] => model.as_str(),
        _ => return Err(USAGE.to_string()),
    };
    let mut body = json!({"model": model, "consent": consent_of(scanned)});
    if scanned.flags.dry_run {
        body["dryRun"] = json!(true);
    }
    Ok((ops::EMBED_REQUEST, body))
}

pub fn done_line(request: &Value) -> String {
    let model = request
        .pointer("/result/model")
        .and_then(Value::as_str)
        .unwrap_or("the model");
    format!("Done: {model} is downloading (the user said yes). It finishes in the background.")
}

pub fn download(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let scanned = match scan(args, USAGE, &[]) {
        Ok(s) => s,
        Err(m) => return Failure::usage(m).report(json),
    };
    let (op, body) = match request(&scanned) {
        Ok(found) => found,
        Err(m) => return Failure::usage(m).report(json),
    };
    through_card(op, body, &scanned, USAGE, |reply, _| {
        done_line(reply.get("request").unwrap_or(reply))
    })
}

#[cfg(test)]
#[path = "semantic_cli_tests.rs"]
mod tests;
