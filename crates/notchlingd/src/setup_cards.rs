use crate::consent_ops::{options, ConsentOps, Opening};
use notchling_engine::bridge::net::crypto::sha256_hex;
use notchling_engine::consent::Consent;
use notchling_local_api::{AgentIdentity, ApiError, Caller, ErrorCode};
use serde_json::{json, Map, Value};

const NO_SESSION: &str = "Only an agent inside a Notchling session may do that.";
const NEVER: &str = "Only you can do this, in Notchling.";
const KEY_REUSED: &str = "That key was used for a different request. Use a new key.";

pub fn forbidden(text: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, text).with_reason(reason)
}

pub fn never() -> ApiError {
    forbidden(NEVER, "never").with_field("never", json!(true))
}

pub fn switch_off(text: &str) -> ApiError {
    forbidden(text, "switch_off")
        .with_field("switch", json!("install"))
        .with_field(
            "ask",
            json!({"command": "notchling switch on install", "kind": "switch"}),
        )
}

pub fn session_of(who: &AgentIdentity) -> Result<String, ApiError> {
    who.session
        .clone()
        .filter(|_| who.verified)
        .ok_or_else(|| forbidden(NO_SESSION, "no_session"))
}

pub fn dry_run(args: &Value) -> bool {
    args.get("dryRun").and_then(Value::as_bool) == Some(true)
}

pub fn shown(consent: &ConsentOps, row: &Consent) -> Value {
    json!({"state": row.state.as_str(), "request": consent.project(row, true)})
}

pub struct Ask<'a> {
    pub kind: &'static str,
    pub op: &'static str,
    pub action: &'a str,
    pub inner: &'a Value,
    pub target: String,
    pub change: Value,
    pub facts: Value,
    pub parts: Vec<Value>,
}

pub fn open(
    consent: &ConsentOps,
    caller: &Caller,
    args: &Value,
    ask: Ask,
) -> Result<Value, ApiError> {
    let (reason, expires_in_ms, key) = options(args);
    let digest = sha256_hex(
        json!({"action": ask.action, "args": ask.inner})
            .to_string()
            .as_bytes(),
    );
    let request = json!({"op": ask.op, "action": ask.action, "digest": digest});
    if !dry_run(args) {
        if let Some(key) = key.as_deref().filter(|k| !k.is_empty()) {
            match consent.earlier(caller, ask.kind, key)? {
                Some(first) if first.request == request => return Ok(shown(consent, &first)),
                Some(_) => return Err(ApiError::bad_request(KEY_REUSED).with_reason("key_reused")),
                None => {}
            }
        }
    }
    let opening = Opening {
        parts: Some(json!(ask.parts)),
        kind: ask.kind,
        target: ask.target,
        request,
        change: ask.change,
        facts: ask.facts,
        reason,
        expires_in_ms,
        key,
    };
    if dry_run(args) {
        let would = consent.would(&opening)?;
        return Ok(json!({"dryRun": true, "op": ask.op, "would": "ask",
            "change": opening.change, "parts": opening.parts, "request": would, "error": null}));
    }
    let opened = consent.open(caller, opening)?;
    Ok(shown(consent, opened.row()))
}

pub struct Taken {
    pub row: Consent,
    pub only: Map<String, Value>,
    pub after: Map<String, Value>,
    pub args: Map<String, Value>,
}

pub fn take(
    consent: &ConsentOps,
    caller: &Caller,
    kind: &str,
    op: &str,
    args: &Value,
) -> Result<Taken, ApiError> {
    ConsentOps::app_only(caller)?;
    let id = args
        .get("consentId")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request("consentId is the card's id"))?;
    let mut given = args.as_object().cloned().unwrap_or_default();
    given.remove("consentId");
    let named = given.remove("parts");
    let stored = consent
        .get_row(id)
        .map_err(|_| forbidden("That request is gone.", "needs_consent"))?;
    if stored.change["apply"] != json!({"op": op, "args": Value::Object(given.clone())}) {
        return Err(forbidden(
            "That request is for something else.",
            "needs_consent",
        ));
    }
    let row =
        consent.take_for_app_keeping(caller, id, kind, Some(&stored.target), named.as_ref())?;
    let names: Vec<String> = named
        .as_ref()
        .and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let kept = match ConsentOps::kept_parts(&row, &names) {
        Ok(kept) => kept,
        Err(e) => {
            consent.withdraw_use(&row);
            return Err(e);
        }
    };
    let mut only = Map::new();
    let mut after = Map::new();
    for id in kept {
        let part = row
            .parts
            .as_ref()
            .and_then(Value::as_array)
            .and_then(|list| list.iter().find(|p| p["id"] == json!(id)))
            .cloned()
            .unwrap_or(Value::Null);
        only.insert(id.clone(), part["before"].clone());
        after.insert(id, part["after"].clone());
        for extra in part["also"].as_array().into_iter().flatten() {
            if let Some(also) = extra["id"].as_str() {
                only.insert(also.to_string(), extra["before"].clone());
            }
        }
    }
    Ok(Taken {
        row,
        only,
        after,
        args: given,
    })
}

pub fn finish(
    consent: &ConsentOps,
    taken: &Taken,
    outcome: Result<Value, ApiError>,
) -> Result<Value, ApiError> {
    match outcome {
        Ok(result) => {
            let stored = json!({
                "action": taken.row.change["action"],
                "done": true,
                "parts": result.get("parts").cloned().unwrap_or(Value::Null),
            });
            consent.finish_use(&taken.row, Ok(stored));
            Ok(result)
        }
        Err(e) => {
            if e.data
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(Value::as_str)
                == Some("stale")
            {
                consent.withdraw_use(&taken.row);
            } else {
                consent.finish_use(&taken.row, Err(e.message.clone()));
            }
            Err(e)
        }
    }
}

pub fn fact(line: &str, args: Value) -> Value {
    json!({"line": line, "args": args})
}
