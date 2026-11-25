use crate::cited::Cited;
use crate::consent_ops::{options, ConsentOps, Handler, Opening};
use crate::embed_fetch::{entry_of, off_error};
use crate::op_group::OpGroup;
use crate::setup_cards::{dry_run, fact, session_of, shown};
use notchling_engine::consent::{Consent, MODEL};
use notchling_engine::search::models::ModelEntry;
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec};
use notchling_locale as t;
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

pub const USAGE: &str = "embed.request takes {\"model\": a listed model id}";
const HERE: &str = "That model is already on this Mac.";
const RUNNING: &str = "That model is already downloading.";
const KEY_REUSED: &str = "That key was used for a different request. Use a new key.";
const MB: u64 = 1_000_000;

pub type Status = Arc<dyn Fn() -> Value + Send + Sync>;

pub struct ModelAsks {
    me: Weak<ModelAsks>,
    consent: Arc<ConsentOps>,
    status: Status,
}

fn forbidden(text: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, text).with_reason(reason)
}

fn bad(text: &str, reason: &str) -> ApiError {
    ApiError::bad_request(text).with_reason(reason)
}

pub fn cites(op: &str, args: &Value) -> bool {
    op == ops::EMBED_FETCH && args.get("consentId").is_some()
}

fn title_of(entry: &ModelEntry) -> String {
    t::lookup(&entry.message)
        .map(|text| text.get().to_owned())
        .unwrap_or_else(|| entry.id.clone())
}

impl ModelAsks {
    pub fn new(consent: Arc<ConsentOps>, status: Status) -> Arc<ModelAsks> {
        let asks = Arc::new_cyclic(|me| ModelAsks {
            me: me.clone(),
            consent: consent.clone(),
            status,
        });
        consent.register(MODEL, Arc::new(Apply(asks.me.clone())));
        asks
    }

    fn check(&self, entry: &ModelEntry) -> Result<(), ApiError> {
        let status = (self.status)();
        if status["state"].as_str() == Some("off") {
            return Err(off_error());
        }
        let present = status["models"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|m| m["id"] == json!(entry.id) && m["present"] == json!(true));
        if present {
            return Err(bad(HERE, "present"));
        }
        let running = status["fetch"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|f| f["model"] == json!(entry.id) && f["state"] == json!("downloading"));
        if running {
            return Err(bad(RUNNING, "running"));
        }
        Ok(())
    }

    fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let who = self.consent.who(caller);
        session_of(&who)?;
        let entry = entry_of(args, ops::EMBED_REQUEST).map_err(|_| ApiError::bad_request(USAGE))?;
        self.check(entry)?;
        let (reason, expires_in_ms, key) = options(args);
        let inner = json!({"model": entry.id});
        let digest =
            notchling_engine::bridge::net::crypto::sha256_hex(inner.to_string().as_bytes());
        let request = json!({"op": ops::EMBED_REQUEST, "action": "download", "digest": digest});
        if !dry_run(args) {
            if let Some(key) = key.as_deref().filter(|k| !k.is_empty()) {
                match self.consent.earlier(caller, MODEL, key)? {
                    Some(first) if first.request == request => {
                        return Ok(shown(&self.consent, &first))
                    }
                    Some(_) => return Err(bad(KEY_REUSED, "key_reused")),
                    None => {}
                }
            }
        }
        let opening = Opening {
            parts: None,
            kind: MODEL,
            target: entry.id.clone(),
            request,
            change: json!({
                "action": "download",
                "model": entry.id,
                "args": inner,
                "apply": {"op": ops::EMBED_FETCH, "args": inner},
            }),
            facts: json!([fact(
                "consent-model-download",
                json!({
                    "model": title_of(entry),
                    "download": (entry.download_bytes / MB).to_string(),
                    "kept": (entry.kept_bytes / MB).to_string(),
                })
            )]),
            reason,
            expires_in_ms,
            key,
        };
        if dry_run(args) {
            let would = self.consent.would(&opening)?;
            return Ok(json!({"dryRun": true, "op": ops::EMBED_REQUEST,
                "would": "ask", "change": opening.change, "request": would, "error": null}));
        }
        let opened = self.consent.open(caller, opening)?;
        Ok(shown(&self.consent, opened.row()))
    }

    fn still_fits(&self, row: &Consent) -> Result<(), String> {
        let id = row.change["model"].as_str().unwrap_or_default();
        let entry = entry_of(&json!({"model": id}), ops::EMBED_FETCH)
            .map_err(|_| "that model is gone".to_string())?;
        self.check(entry).map_err(|e| e.message)
    }

    pub fn apply(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
        run: &dyn Fn(&Value) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        ConsentOps::app_only(caller)?;
        let id = args
            .get("consentId")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request(USAGE))?;
        let mut given = args.clone();
        if let Some(map) = given.as_object_mut() {
            map.remove("consentId");
        }
        let stored = self
            .consent
            .get_row(id)
            .map_err(|_| forbidden("That request is gone.", "needs_consent"))?;
        if stored.change["apply"] != json!({"op": op, "args": given}) {
            return Err(forbidden(
                "That request is for something else.",
                "needs_consent",
            ));
        }
        let row = self
            .consent
            .take_for_app(caller, id, MODEL, Some(&stored.target))?;
        if let Err(why) = self.still_fits(&row) {
            self.consent.withdraw_use(&row);
            return Err(forbidden(&format!("It no longer fits: {why}."), "stale"));
        }
        match run(&given) {
            Ok(result) => {
                self.consent.finish_use(
                    &row,
                    Ok(json!({"action": "download", "model": row.change["model"], "done": true})),
                );
                Ok(result)
            }
            Err(e) => {
                self.consent.finish_use(&row, Err(e.message.clone()));
                Err(e)
            }
        }
    }
}

struct Apply(Weak<ModelAsks>);

impl Handler for Apply {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) => asks.still_fits(row),
            None => Err("stopping".to_string()),
        }
    }
}

impl OpGroup for ModelAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::EMBED_REQUEST).then_some(OpSpec::AGENT_ACTS)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::EMBED_REQUEST => self.request(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

impl Cited for ModelAsks {
    fn claims(&self, op: &str, args: &Value) -> bool {
        cites(op, args)
    }

    fn apply(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
        run: &dyn Fn(&Value) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        ModelAsks::apply(self, caller, op, args, run)
    }
}

#[cfg(test)]
#[path = "model_asks_tests.rs"]
mod tests;
