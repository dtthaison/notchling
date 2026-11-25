use crate::acting_ops::now_ms;
use crate::consent_ops::{options, ConsentOps, Handler, Opening};
use crate::op_group::OpGroup;
use notchling_engine::bridge::net::crypto::sha256_hex;
use notchling_engine::consent::{Consent, CLUSTER};
use notchling_engine::organizations::PERSONAL_ID;
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

pub const ACTIONS: [&str; 10] = [
    "create",
    "token",
    "invite",
    "member",
    "failover",
    "main",
    "allow",
    "remove",
    "move_home",
    "move_now",
];
pub const USER_ONLY: [&str; 3] = ["promote", "take_back", "continue_here"];
pub const APP_OPS: [&str; 10] = [
    ops::CLUSTER_CREATE,
    ops::CLUSTER_JOIN_TOKEN,
    ops::CLUSTER_INVITE,
    ops::CLUSTER_MEMBER_SET,
    ops::CLUSTER_FAILOVER_SET,
    ops::CLUSTER_MAIN_SET,
    ops::CLUSTER_ALLOW_SET,
    ops::CLUSTER_REMOVE,
    ops::CLUSTER_MOVE_HOME,
    ops::CLUSTER_MOVE_NOW,
];
pub const USAGE: &str = "cluster.admin.request takes {\"action\": \"create\"|\"token\"|\"invite\"|\"member\"|\"failover\"|\"main\"|\"allow\"|\"remove\"|\"move_home\"|\"move_now\", \"args\": {...}}";
pub const HANDOVER_MS: i64 = 11 * 60_000;
const CLUSTER_OFF: &str = "Cluster is off, so no agent may change it.";
const OTHER_ORG: &str = "That machine or secret is in another organization.";
const NO_SESSION: &str = "Only an agent inside a Notchling session may do that.";
const NEVER: &str = "Only you can do this, in Notchling.";
const NO_CLUSTER: &str = "This machine is in no cluster.";
const NAME_CHARS: usize = 64;
const ALLOW_MAX: usize = 16;

const UNKNOWN_OWNER: &str = "Notchling can't tell which organization that machine is in, so only the user can change it, in the app.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Probe,
    Stored,
}

pub trait Admin: Send + Sync {
    fn status(&self) -> Value;
    fn own_organization(&self) -> Option<String>;
    fn organization_of(&self, machine: &str) -> Option<String>;
    fn host_of(&self, machine: &str) -> Option<String>;
    fn matches(&self, machine: &str) -> usize;
    fn node_of(&self, machine: &str) -> Option<String>;
    fn secret_organization(&self, name: &str) -> Option<String>;
}

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
pub type SwitchOn = Arc<dyn Fn() -> bool + Send + Sync>;

struct Held {
    session: String,
    value: Value,
    at_ms: i64,
}

pub struct Plan {
    pub action: &'static str,
    pub target: String,
    pub change: Value,
    pub facts: Value,
}

pub struct ClusterAsks {
    me: Weak<ClusterAsks>,
    consent: Arc<ConsentOps>,
    admin: Arc<dyn Admin>,
    cluster_on: SwitchOn,
    clock: Clock,
    held: Mutex<HashMap<String, Held>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn bad(text: &str) -> ApiError {
    ApiError::bad_request(text)
}

fn forbidden(text: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, text).with_reason(reason)
}

fn unknown_owner() -> ApiError {
    forbidden(UNKNOWN_OWNER, "unknown_owner")
}

fn dry_run(args: &Value) -> bool {
    args.get("dryRun").and_then(Value::as_bool) == Some(true)
}

fn cluster_off() -> ApiError {
    forbidden(CLUSTER_OFF, "switch_off")
        .with_field("switch", json!("cluster"))
        .with_field(
            "ask",
            json!({"command": "notchling switch on cluster", "kind": "switch"}),
        )
}

fn caller_of(who: &AgentIdentity) -> Result<(String, String), ApiError> {
    let session = who
        .session
        .clone()
        .filter(|_| who.verified)
        .ok_or_else(|| forbidden(NO_SESSION, "no_session"))?;
    let organization = who
        .organization
        .clone()
        .unwrap_or_else(|| PERSONAL_ID.to_string());
    Ok((session, organization))
}

pub fn cites(op: &str, args: &Value) -> bool {
    APP_OPS.contains(&op) && args.get("consentId").is_some_and(Value::is_string)
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn flag(args: &Value, key: &str) -> Result<Option<bool>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| bad(USAGE)),
    }
}

fn known_keys(args: &Value, keys: &[&str]) -> Result<(), ApiError> {
    match args.as_object() {
        Some(map) if map.keys().all(|k| keys.contains(&k.as_str())) => Ok(()),
        _ => Err(bad(USAGE)),
    }
}

fn role_word(can_be_master: bool) -> &'static str {
    if can_be_master {
        "master"
    } else {
        "worker"
    }
}

fn on_word(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

fn failover_value(word: &str) -> Option<&'static str> {
    match word {
        "on" | "30m" => Some("30m"),
        "off" => Some("off"),
        "1h" => Some("1h"),
        _ => None,
    }
}

impl ClusterAsks {
    pub fn new(
        consent: Arc<ConsentOps>,
        admin: Arc<dyn Admin>,
        cluster_on: SwitchOn,
    ) -> Arc<ClusterAsks> {
        Self::with_clock(consent, admin, cluster_on, Arc::new(now_ms))
    }

    pub fn with_clock(
        consent: Arc<ConsentOps>,
        admin: Arc<dyn Admin>,
        cluster_on: SwitchOn,
        clock: Clock,
    ) -> Arc<ClusterAsks> {
        let asks = Arc::new_cyclic(|me| ClusterAsks {
            me: me.clone(),
            consent: consent.clone(),
            admin,
            cluster_on,
            clock,
            held: Mutex::new(HashMap::new()),
        });
        consent.register(CLUSTER, Arc::new(Apply(asks.me.clone())));
        asks
    }

    pub fn claims(&self, op: &str, args: &Value) -> bool {
        cites(op, args)
            && !(op == ops::CLUSTER_MEMBER_SET
                && args.get("desired").is_some_and(Value::is_object)
                && !self.cites(op, args))
    }

    pub fn cites(&self, op: &str, args: &Value) -> bool {
        cites(op, args)
            && args
                .get("consentId")
                .and_then(Value::as_str)
                .and_then(|id| self.consent.get_row(id).ok())
                .is_some_and(|row| row.kind == CLUSTER)
    }

    fn gate(&self) -> Result<(), ApiError> {
        if (self.cluster_on)() {
            Ok(())
        } else {
            Err(cluster_off())
        }
    }

    pub fn plan(
        &self,
        action: &str,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        match action {
            "create" => self.plan_create(args, mine),
            "token" => self.plan_token(args, mine, look),
            "invite" => self.plan_invite(args, mine, look),
            "member" => self.plan_member(args, mine, look),
            "failover" => self.plan_failover(args, mine, look),
            "main" => self.plan_main(args, mine, look),
            "allow" => self.plan_allow(args, mine, look),
            "remove" => self.plan_remove(args, mine, look),
            "move_home" => self.plan_move_home(args, mine, look),
            "move_now" => self.plan_move_now(args, mine, look),
            _ => Err(bad(USAGE)),
        }
    }

    fn same_cluster(&self, change: &Value) -> bool {
        let status = self.admin.status();
        if change["clusterId"].is_null() {
            return status["cluster"].is_null() && status["joining"].is_null();
        }
        status["cluster"]["id"] == change["clusterId"]
    }

    fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| bad(USAGE))?;
        if USER_ONLY.contains(&action) {
            return Err(forbidden(NEVER, "never").with_field("never", json!(true)));
        }
        if !ACTIONS.contains(&action) {
            return Err(bad(USAGE));
        }
        let who = self.consent.who(caller);
        let (_, mine) = caller_of(&who)?;
        self.gate()?;
        let inner = match args.get("args") {
            None | Some(Value::Null) => json!({}),
            Some(v) if v.is_object() => v.clone(),
            Some(_) => return Err(bad(USAGE)),
        };
        let (reason, expires_in_ms, key) = options(args);
        let digest = sha256_hex(
            json!({"action": action, "args": inner})
                .to_string()
                .as_bytes(),
        );
        let request = json!({"op": ops::CLUSTER_ADMIN_REQUEST, "action": action, "digest": digest});
        if !dry_run(args) {
            if let Some(first) = self.earlier(caller, key.as_deref(), &request)? {
                return Ok(Self::shown(&self.consent, &first));
            }
        }
        if !dry_run(args) {
            self.name_refusal(action, &inner)?;
        }
        let plan = self.plan(action, &inner, &mine, Look::Probe)?;
        if !self.same_cluster(&plan.change) {
            return Err(
                bad("The cluster changed while it was checked. Ask again.").with_reason("stale")
            );
        }
        let opening = Opening {
            parts: None,
            kind: CLUSTER,
            target: plan.target.clone(),
            request,
            change: plan.change,
            facts: plan.facts,
            reason,
            expires_in_ms,
            key,
        };
        if dry_run(args) {
            let would = self.consent.would(&opening)?;
            return Ok(json!({"dryRun": true, "op": ops::CLUSTER_ADMIN_REQUEST,
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
        match self.consent.earlier(caller, CLUSTER, key)? {
            Some(first) if first.request == *request => Ok(Some(first)),
            Some(_) => Err(
                bad("That key was used for a different request. Use a new key.")
                    .with_reason("key_reused"),
            ),
            None => Ok(None),
        }
    }

    fn still_fits(&self, row: &Consent) -> Result<(), String> {
        if !(self.cluster_on)() {
            return Err("Cluster is off".to_string());
        }
        let mine = row
            .organization
            .clone()
            .unwrap_or_else(|| PERSONAL_ID.to_string());
        let action = row.change["action"].as_str().unwrap_or_default();
        let args = row.change.get("args").cloned().unwrap_or(Value::Null);
        let now = self
            .plan(action, &args, &mine, Look::Stored)
            .map_err(|e| e.message.clone())?;
        if now.change == row.change && now.facts == row.facts {
            Ok(())
        } else {
            Err("the cluster changed since the request".to_string())
        }
    }

    fn keep(&self, row: &Consent, result: &Value) -> Value {
        let mut shown = result.clone();
        let token = shown
            .as_object_mut()
            .and_then(|map| map.remove("token"))
            .filter(|t| t.is_string());
        if let (Some(token), Some(session)) = (token, row.session.clone().filter(|_| row.verified))
        {
            let value = json!({"token": token, "expiresMs": result.get("expiresMs")});
            let mut held = lock(&self.held);
            let now = (self.clock)();
            held.retain(|_, h| now - h.at_ms < HANDOVER_MS);
            held.insert(
                row.id.clone(),
                Held {
                    session,
                    value,
                    at_ms: now,
                },
            );
        }
        shown
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
        if stored.change["apply"] != json!({"op": op, "args": given}) {
            return Err(forbidden(
                "That request is for something else.",
                "needs_consent",
            ));
        }
        let row = self
            .consent
            .take_for_app(caller, id, CLUSTER, Some(&stored.target))?;
        let fits = self.still_fits(&row).and_then(|()| {
            if self.same_cluster(&row.change) {
                Ok(())
            } else {
                Err("the cluster changed since the request".to_string())
            }
        });
        if let Err(why) = fits {
            self.consent.withdraw_use(&row);
            return Err(forbidden(&format!("It no longer fits: {why}."), "stale"));
        }
        let outcome = run(&given);
        match outcome {
            Ok(result) => {
                let shown = self.keep(&row, &result);
                let action = row.change["action"].clone();
                self.consent
                    .finish_use(&row, Ok(json!({"action": action, "done": true})));
                Ok(shown)
            }
            Err(e) => {
                self.consent.finish_use(&row, Err(e.message.clone()));
                Err(e)
            }
        }
    }

    fn take_held(&self, row: &Consent, who: &AgentIdentity) -> Option<Value> {
        let mut held = lock(&self.held);
        let now = (self.clock)();
        held.retain(|_, h| now - h.at_ms < HANDOVER_MS);
        let session = who.session.as_deref().filter(|_| who.verified)?;
        match held.get(&row.id) {
            Some(h) if h.session == session => held.remove(&row.id).map(|h| h.value),
            _ => None,
        }
    }
}

struct Apply(Weak<ClusterAsks>);

impl Handler for Apply {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) => asks.still_fits(row),
            None => Err("stopping".to_string()),
        }
    }

    fn handover(&self, row: &Consent, who: &AgentIdentity) -> Option<Value> {
        self.0.upgrade()?.take_held(row, who)
    }
}

impl OpGroup for ClusterAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::CLUSTER_ADMIN_REQUEST).then_some(OpSpec::AGENT_ACTS)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::CLUSTER_ADMIN_REQUEST => self.request(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[path = "cluster_asks_plans.rs"]
mod plans;

#[cfg(test)]
#[path = "cluster_asks_tests.rs"]
mod tests;
