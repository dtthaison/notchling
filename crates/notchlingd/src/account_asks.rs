use crate::consent_ops::{options, ConsentOps, Handler, Opening};
use crate::op_group::OpGroup;
use notchling_engine::accounts::claude::Snapshot;
use notchling_engine::bridge::net::crypto::sha256_hex;
use notchling_engine::consent::{Consent, ACCOUNT};
use notchling_engine::organizations::{Kind, Resolved};
use notchling_engine::providers::ProviderState;
use notchling_engine::sessions::{Agent, Lane};
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

pub const ACTIONS: [&str; 8] = [
    "switch",
    "add",
    "remove",
    "move_all",
    "move",
    "lane_create",
    "lane_remove",
    "lane_switch",
];
pub const APP_OPS: [&str; 11] = [
    ops::ACCOUNTS_SWITCH,
    ops::ACCOUNTS_ADD_BEGIN,
    ops::ACCOUNTS_REMOVE,
    ops::ACCOUNTS_MOVE_ALL,
    ops::LANES_CREATE,
    ops::LANES_REMOVE,
    ops::LANES_SWITCH,
    ops::PROVIDERS_SWITCH,
    ops::PROVIDERS_ADD_BEGIN,
    ops::PROVIDERS_REMOVE,
    ops::SESSION_MOVE,
];
pub const USAGE: &str = "accounts.request takes {\"action\": \"switch\"|\"add\"|\"remove\"|\"move_all\"|\"move\"|\"lane_create\"|\"lane_remove\"|\"lane_switch\", \"args\": {...}}";
pub const NO_SESSION: &str = "Run this from a session in the organization's folder.";
pub const NEVER: &str = "Only you can do this, in Notchling.";
pub const UNKNOWN_OWNER: &str = "Notchling can't tell which organization that account is in, so only the user can change it, in the app.";
pub const ACCOUNTS_OFF: &str = "Accounts are off, so no agent may change them.";
pub const ACTING_OFF: &str = "Acting is off, so no agent may move sessions.";
pub const KEY_REUSED: &str = "That key was used for a different request. Use a new key.";

pub trait Book: Send + Sync {
    fn claude(&self) -> Snapshot;
    fn lanes(&self) -> Vec<Lane>;
    fn providers(&self) -> Vec<(String, ProviderState)>;
    fn agents(&self) -> Vec<Agent>;
    fn closed(&self) -> bool;
    fn listing(&self, kind: Kind) -> Option<Vec<(String, Option<String>)>>;
    fn resolve(&self, kind: Kind, key: &str, field: Option<&str>) -> Resolved;
    fn folder_org(&self, folder: &str) -> String;
    fn main_dir(&self) -> String;
    fn home(&self) -> String;
    fn accounts_on(&self) -> bool;
    fn acting_on(&self) -> bool;
    fn can_add(&self, provider: &str) -> bool;
    fn add_limit_ms(&self) -> i64;
    fn added(&self, attempt: u64) -> Option<Vec<String>>;
    fn id(&self, provider: &str, raw: &str) -> Option<String>;
    fn lane_id(&self, name: &str) -> Option<String>;
    fn place(&self, kind: Kind, raw: &str, organization: &str) -> bool;
}

pub struct Plan {
    pub target: String,
    pub change: Value,
    pub facts: Value,
}

pub struct AccountAsks {
    consent: Arc<ConsentOps>,
    book: Arc<dyn Book>,
    follow: Arc<follow::AccountFollow>,
}

pub(crate) fn bad(text: &str) -> ApiError {
    ApiError::bad_request(text)
}

pub(crate) fn bad_why(text: &str, reason: &str) -> ApiError {
    ApiError::bad_request(text).with_reason(reason)
}

pub(crate) fn forbidden(text: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, text).with_reason(reason)
}

pub(crate) fn never() -> ApiError {
    forbidden(NEVER, "never").with_field("never", json!(true))
}

pub(crate) fn unknown_owner() -> ApiError {
    forbidden(UNKNOWN_OWNER, "unknown_owner")
}

pub(crate) fn switch_off(name: &str, text: &str) -> ApiError {
    forbidden(text, "switch_off")
        .with_field("switch", json!(name))
        .with_field(
            "ask",
            json!({"command": format!("notchling switch on {name}"), "kind": "switch"}),
        )
}

fn dry_run(args: &Value) -> bool {
    args.get("dryRun").and_then(Value::as_bool) == Some(true)
}

pub(crate) fn caller_of(who: &AgentIdentity) -> Result<String, ApiError> {
    match (&who.session, &who.organization) {
        (Some(_), Some(org)) if who.verified => Ok(org.clone()),
        _ => Err(forbidden(NO_SESSION, "no_session")),
    }
}

pub fn cites(op: &str, args: &Value) -> bool {
    APP_OPS.contains(&op) && args.get("consentId").is_some_and(Value::is_string)
}

impl AccountAsks {
    pub fn new(
        consent: Arc<ConsentOps>,
        book: Arc<dyn Book>,
        follow: Arc<follow::AccountFollow>,
    ) -> Arc<AccountAsks> {
        let asks = Arc::new(AccountAsks {
            consent: consent.clone(),
            book,
            follow: follow.clone(),
        });
        let weak = Arc::downgrade(&asks);
        follow.bind(Arc::new(move |entry, end| {
            if let Some(asks) = weak.upgrade() {
                asks.settle(entry, end);
            }
        }));
        consent.register(ACCOUNT, Arc::new(Apply(Arc::downgrade(&asks))));
        asks
    }

    pub fn follow(&self) -> &Arc<follow::AccountFollow> {
        &self.follow
    }

    pub fn claims(&self, op: &str, args: &Value) -> bool {
        cites(op, args)
    }

    fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .filter(|a| ACTIONS.contains(a))
            .ok_or_else(|| bad_why(USAGE, "usage"))?;
        let who = self.consent.who(caller);
        let mine = caller_of(&who)?;
        let inner = args.get("args").cloned().unwrap_or(Value::Null);
        plans::shape(action, &inner)?;
        let (reason, expires_in_ms, key) = options(args);
        if reason.as_ref().is_some_and(|r| r.chars().count() > 300) {
            return Err(bad_why(USAGE, "usage"));
        }
        let digest = sha256_hex(
            json!({"action": action, "args": inner})
                .to_string()
                .as_bytes(),
        );
        let request = json!({"op": ops::ACCOUNTS_REQUEST, "action": action, "digest": digest});
        if !dry_run(args) {
            if let Some(first) = self.earlier(caller, key.as_deref(), &request)? {
                return Ok(Self::shown(&self.consent, &first));
            }
        }
        let plan = self.plan(action, &inner, &mine)?;
        let opening = Opening {
            parts: None,
            kind: ACCOUNT,
            target: plan.target,
            request,
            change: plan.change,
            facts: plan.facts,
            reason,
            expires_in_ms,
            key,
        };
        if dry_run(args) {
            let would = self.consent.would(&opening)?;
            return Ok(json!({"dryRun": true, "op": ops::ACCOUNTS_REQUEST,
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
        match self.consent.earlier(caller, ACCOUNT, key)? {
            Some(first) if first.request == *request => Ok(Some(first)),
            Some(_) => Err(bad_why(KEY_REUSED, "key_reused")),
            None => Ok(None),
        }
    }

    fn still_fits(&self, row: &Consent) -> Result<(), String> {
        let Some(mine) = row.organization.clone() else {
            return Err("the request has no organization".to_string());
        };
        let action = row.change["action"].as_str().unwrap_or_default();
        let args = row.change.get("args").cloned().unwrap_or(Value::Null);
        let now = self
            .plan(action, &args, &mine)
            .map_err(|e| e.message.trim_end_matches('.').to_string())?;
        if now.change == row.change && now.facts == row.facts {
            Ok(())
        } else {
            Err("the accounts changed since the request".to_string())
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
            .ok_or_else(|| bad(USAGE))?;
        let mut given = args.clone();
        if let Some(map) = given.as_object_mut() {
            map.remove("consentId");
        }
        let stored = self
            .consent
            .get_row(id)
            .map_err(|_| forbidden("That request is gone.", "needs_consent"))?;
        if stored.kind != ACCOUNT || stored.change["apply"] != json!({"op": op, "args": given}) {
            return Err(forbidden(
                "That request is for something else.",
                "needs_consent",
            ));
        }
        let row = self
            .consent
            .take_for_app(caller, id, ACCOUNT, Some(&stored.target))?;
        if let Err(why) = self.still_fits(&row) {
            self.consent.withdraw_use(&row);
            return Err(forbidden(&format!("It no longer fits: {why}."), "stale"));
        }
        let Some(raw) = self.translate(&row.change) else {
            self.consent.withdraw_use(&row);
            return Err(forbidden(
                "It no longer fits: the account or lane is gone.",
                "stale",
            ));
        };
        self.follow.hold(&row.id, self.adds_of(&row));
        let watch = self.watch_before(&row);
        let started = self.consent.now();
        match run(&raw) {
            Ok(reply) => {
                self.after_run(&row, watch, &reply, started);
                Ok(reply)
            }
            Err(e) => {
                self.follow.forget(&row.id);
                self.consent.finish_use(&row, Err(e.message.clone()));
                Err(e)
            }
        }
    }
}

struct Apply(Weak<AccountAsks>);

impl Handler for Apply {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) => asks.still_fits(row),
            None => Err("stopping".to_string()),
        }
    }

    fn tick(&self, now_ms: i64) {
        if let Some(asks) = self.0.upgrade() {
            asks.follow.tick(now_ms);
        }
    }
}

impl OpGroup for AccountAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::ACCOUNTS_REQUEST => Some(OpSpec::AGENT_ACTS),
            ops::LANES_SUMMARY => Some(OpSpec::AGENT_READ),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::ACCOUNTS_REQUEST => self.request(caller, args),
            ops::LANES_SUMMARY => Ok(self.lanes_summary(caller)),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[path = "account_asks_plans.rs"]
mod plans;

#[path = "account_asks_follow.rs"]
pub mod follow;

#[cfg(test)]
#[path = "account_asks_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "account_asks_rig.rs"]
mod rig;

#[cfg(test)]
#[path = "account_asks_follow_tests.rs"]
mod follow_tests;

#[cfg(test)]
#[path = "account_asks_harness_tests.rs"]
mod harness_tests;

#[cfg(test)]
#[path = "account_asks_owner_tests.rs"]
mod owner_tests;
