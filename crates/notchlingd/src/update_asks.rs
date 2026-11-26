use crate::cited::Cited;
use crate::consent_ops::{options, retry_of, ConsentOps, Handler, Opening};
use crate::op_group::OpGroup;
use crate::versions_ops::VersionsOps;
use notchling_engine::consent::retry::unretried;
use notchling_engine::consent::{Consent, UPDATE};
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

pub const USAGE: &str = "versions.request takes {\"tool\": \"claude\"|\"codex\"|\"agy\", \"to\": <the version to go back to>}";
pub const WHERE: &str = "mac";
const INSTALL_OFF: &str = "Install is off, so no agent may update a tool.";
const NO_SESSION: &str = "Only an agent inside a Notchling session may do that.";
const USE_MACHINES: &str = "A machine's tools are updated with machines.request and the action update, not with versions.request.";
const NOT_INSTALLED: &str = "That tool isn't installed on this Mac.";
const NOT_GO_BACK: &str = "That isn't the version to go back to.";
const UP_TO_DATE: &str = "That tool is already the newest version.";
const RUNNING: &str = "That tool is already being updated.";
const NOTCHLING_SELF: &str = "Notchling updates itself.";

pub type SwitchOn = Arc<dyn Fn() -> bool + Send + Sync>;

pub struct Plan {
    pub action: &'static str,
    pub target: String,
    pub change: Value,
    pub facts: Value,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Probe,
    Stored,
}

pub struct UpdateAsks {
    me: Weak<UpdateAsks>,
    consent: Arc<ConsentOps>,
    versions: Arc<VersionsOps>,
    install_on: SwitchOn,
}

fn bad(text: &str, reason: &str) -> ApiError {
    ApiError::bad_request(text).with_reason(reason)
}

fn forbidden(text: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, text).with_reason(reason)
}

fn install_off() -> ApiError {
    forbidden(INSTALL_OFF, "switch_off")
        .with_field("switch", json!("install"))
        .with_field(
            "ask",
            json!({"command": "notchling switch on install", "kind": "switch"}),
        )
}

fn dry_run(args: &Value) -> bool {
    args.get("dryRun").and_then(Value::as_bool) == Some(true)
}

fn session_of(who: &AgentIdentity) -> Result<(), ApiError> {
    who.session
        .as_ref()
        .filter(|_| who.verified)
        .map(|_| ())
        .ok_or_else(|| forbidden(NO_SESSION, "no_session"))
}

pub fn cites(op: &str, args: &Value) -> bool {
    op == ops::VERSIONS_UPDATE_LOCAL && args.get("consentId").is_some()
}

impl UpdateAsks {
    pub fn new(
        consent: Arc<ConsentOps>,
        versions: Arc<VersionsOps>,
        install_on: SwitchOn,
    ) -> Arc<UpdateAsks> {
        let asks = Arc::new_cyclic(|me| UpdateAsks {
            me: me.clone(),
            consent: consent.clone(),
            versions,
            install_on,
        });
        consent.register(UPDATE, Arc::new(Apply(asks.me.clone())));
        asks
    }

    fn gate(&self) -> Result<(), ApiError> {
        if (self.install_on)() {
            Ok(())
        } else {
            Err(install_off())
        }
    }

    fn asked(args: &Value) -> Result<(String, Option<String>), ApiError> {
        let tool = args
            .get("tool")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| ApiError::bad_request(USAGE))?;
        let to = match args.get("to") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) if !text.trim().is_empty() => Some(text.trim().to_string()),
            Some(_) => return Err(ApiError::bad_request(USAGE)),
        };
        match args.get("machine") {
            None | Some(Value::Null) => {}
            Some(_) => return Err(bad(USE_MACHINES, "use_machines")),
        }
        Ok((tool.to_string(), to))
    }

    fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let who = self.consent.who(caller);
        session_of(&who)?;
        self.gate()?;
        let (tool, to) = Self::asked(args)?;
        let (reason, expires_in_ms, key) = options(args);
        let digest = notchling_engine::bridge::net::crypto::sha256_hex(
            json!({"tool": tool, "to": to}).to_string().as_bytes(),
        );
        let plan = self.plan(&tool, to.as_deref(), Look::Probe)?;
        let request = json!({"op": ops::VERSIONS_REQUEST, "action": plan.action, "digest": digest});
        if !dry_run(args) {
            if let Some(first) = self.earlier(caller, key.as_deref(), &request)? {
                return Ok(Self::shown(&self.consent, &first));
            }
        }
        let opening = Opening {
            parts: None,
            kind: UPDATE,
            target: plan.target,
            request,
            change: plan.change,
            facts: plan.facts,
            reason,
            expires_in_ms,
            key,
            retry_of: retry_of(args),
        };
        if dry_run(args) {
            let would = self.consent.would(&opening)?;
            return Ok(json!({"dryRun": true, "op": ops::VERSIONS_REQUEST,
                "would": "ask", "change": opening.change, "request": would, "error": null}));
        }
        let opened = self.consent.open(caller, opening)?;
        Ok(Self::shown(&self.consent, opened.row()))
    }

    fn shown(consent: &ConsentOps, row: &Consent) -> Value {
        json!({"state": row.state.as_str(), "request": consent.project(row, true)})
    }

    fn earlier(
        &self,
        caller: &Caller,
        key: Option<&str>,
        request: &Value,
    ) -> Result<Option<Consent>, ApiError> {
        let Some(key) = key.filter(|k| !k.is_empty()) else {
            return Ok(None);
        };
        match self.consent.earlier(caller, UPDATE, key)? {
            Some(first) if first.request == *request => Ok(Some(first)),
            Some(_) => Err(bad(
                "That key was used for a different request. Use a new key.",
                "key_reused",
            )),
            None => Ok(None),
        }
    }

    fn still_fits(&self, row: &Consent) -> Result<(), String> {
        if !(self.install_on)() {
            return Err("Install is off".to_string());
        }
        let tool = row.change["tool"].as_str().unwrap_or_default();
        let to = row.change["apply"]["args"]["to"].as_str();
        let now = self
            .plan(tool, to, Look::Stored)
            .map_err(|e| e.message.clone())?;
        if now.change == unretried(&row.change) && now.facts == row.facts {
            Ok(())
        } else {
            Err("the tool changed since the request".to_string())
        }
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
            .take_for_app(caller, id, UPDATE, Some(&stored.target))?;
        if let Err(why) = self.still_fits(&row) {
            self.consent.withdraw_use(&row);
            return Err(forbidden(&format!("It no longer fits: {why}."), "stale"));
        }
        match run(&given) {
            Ok(result) => {
                let (action, tool) = (row.change["action"].clone(), row.change["tool"].clone());
                self.consent.finish_use(
                    &row,
                    Ok(json!({"action": action, "tool": tool, "done": true})),
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

struct Apply(Weak<UpdateAsks>);

impl Handler for Apply {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) => asks.still_fits(row),
            None => Err("stopping".to_string()),
        }
    }
}

impl OpGroup for UpdateAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::VERSIONS_REQUEST).then_some(OpSpec::AGENT_ACTS)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::VERSIONS_REQUEST => self.request(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

impl Cited for UpdateAsks {
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
        UpdateAsks::apply(self, caller, op, args, run)
    }
}

#[path = "update_asks_plan.rs"]
mod plan;

#[cfg(test)]
#[path = "update_asks_tests.rs"]
mod tests;
