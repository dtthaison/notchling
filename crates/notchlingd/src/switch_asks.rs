use crate::acting_ops::{now_ms, ActingOps};
use crate::bridge_ops::BridgeOps;
use crate::consent_ops::{options, ConsentOps, Handler, Opening};
use crate::inbox_ops::{InboxOps, Responder};
use crate::op_group::OpGroup;
use crate::switch_worker::{Place, Where};
use crate::switches::SwitchOps;
use notchling_engine::consent::{Consent, SWITCH};
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use notchling_local_api::{ops, ApiError, Caller, ClientKind, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock, Weak};

pub const NAMES: [&str; 7] = [
    "acting", "bridge", "accounts", "vault", "install", "cluster", "machines",
];
pub const ALREADY_ON: &str = "It is already on.";
const ONLY_ON: &str = "A request only turns a switch on.";
const REQUEST_USAGE: &str = "switch.request takes {\"switch\": \"acting\"|\"bridge\"|\"accounts\"|\"vault\"|\"install\"|\"cluster\"|\"machines\", \"on\": true}";
const OFF_USAGE: &str = "switch.off takes {\"switch\": \"acting\"|\"bridge\"|\"accounts\"|\"vault\"|\"install\"|\"cluster\"|\"machines\"}";
const APP_OPS: [&str; 3] = [
    ops::SETTINGS_SET_ACTING,
    ops::BRIDGE_SET,
    ops::SETTINGS_SET_SWITCH,
];

pub type States = Arc<dyn Fn(&str) -> Option<bool> + Send + Sync>;
pub type Turn = Arc<dyn Fn(&str, bool, &Value) -> Result<Value, ApiError> + Send + Sync>;
pub type Headless = Arc<dyn Fn() -> bool + Send + Sync>;

pub struct SwitchAsks {
    consent: Arc<ConsentOps>,
    states: States,
    turn: Turn,
    inbox: Option<Arc<InboxOps>>,
    place: OnceLock<Where>,
}

fn bad(text: &str) -> ApiError {
    ApiError::bad_request(text)
}

fn name_of<'a>(args: &'a Value, usage: &str) -> Result<&'a str, ApiError> {
    args.get("switch")
        .and_then(Value::as_str)
        .filter(|name| NAMES.contains(name))
        .ok_or_else(|| bad(usage))
}

pub fn cites(op: &str, args: &Value) -> bool {
    APP_OPS.contains(&op) && args.get("consentId").is_some_and(|id| !id.is_null())
}

fn switch_title(name: &str) -> &'static str {
    match name {
        "acting" => "Act on Sessions",
        "bridge" => "the Phone Bridge",
        "accounts" => "Accounts",
        "vault" => "the Vault",
        "install" => "Install",
        "machines" => "Machines",
        _ => "the cluster",
    }
}

fn tool_title(tool: &str) -> &'static str {
    match tool {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "antigravity" => "Antigravity",
        _ => "An agent",
    }
}

pub fn off_notice(name: &str, tool: &str, session: Option<&str>, created_ms: i64) -> Item {
    Item {
        id: format!("switch-off:{name}:{created_ms}"),
        kind: Kind::Notice,
        title: format!("{} turned off {}", tool_title(tool), switch_title(name)),
        detail: "It stays off until you turn it on.".to_string(),
        source: Source {
            node: None,
            session: session.map(str::to_string),
            job: None,
        },
        created_ms,
        expires_ms: None,
        actions: vec![Action {
            id: ActionId::Continue,
            label: "Turn On".to_string(),
            primary: true,
        }],
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: Some(json!({"switch": name})),
        closed_ms: None,
        switch: None,
    }
}

fn the_app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

pub fn turn_for(acting: Arc<ActingOps>, bridge: Arc<BridgeOps>, switches: Arc<SwitchOps>) -> Turn {
    Arc::new(move |name: &str, on: bool, extra: &Value| {
        let mut args = extra.clone();
        if !args.is_object() {
            args = json!({});
        }
        args["on"] = json!(on);
        match name {
            "acting" => acting.call(&the_app(), ops::SETTINGS_SET_ACTING, &args),
            "bridge" => bridge.call(ops::BRIDGE_SET, &args),
            other => {
                args["switch"] = json!(other);
                switches.call(&the_app(), ops::SETTINGS_SET_SWITCH, &args)
            }
        }
    })
}

struct TurnOn {
    name: String,
    turn: Turn,
    headless: Headless,
}

impl Responder for TurnOn {
    fn answered(&self, _item: &Item, _action: ActionId) -> bool {
        false
    }

    fn answered_by(&self, _item: &Item, action: ActionId, by: &str) -> bool {
        action == ActionId::Continue
            && by == "mac"
            && !(self.headless)()
            && (self.turn)(&self.name, true, &json!({})).is_ok()
    }
}

impl SwitchAsks {
    pub fn new(
        consent: Arc<ConsentOps>,
        states: States,
        turn: Turn,
        inbox: Option<Arc<InboxOps>>,
    ) -> Arc<SwitchAsks> {
        let asks = Arc::new(SwitchAsks {
            consent: consent.clone(),
            states,
            turn,
            inbox,
            place: OnceLock::new(),
        });
        consent.register(SWITCH, Arc::new(Recheck(Arc::downgrade(&asks))));
        asks
    }

    pub fn set_place(&self, place: Where) {
        let _ = self.place.set(place);
    }

    fn place(&self) -> Place {
        self.place.get().map_or(Place::Mac, |place| place())
    }

    fn headless(&self) -> Headless {
        let (place, consent) = (self.place.get().cloned(), self.consent.clone());
        Arc::new(move || {
            let here = place.as_ref().map_or(Place::Mac, |place| place());
            crate::switch_worker::headless(&here, consent.app_attached())
        })
    }

    fn is_on(&self, name: &str) -> bool {
        (self.states)(name) == Some(true)
    }

    fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let name = name_of(args, REQUEST_USAGE)?;
        if args.get("on").and_then(Value::as_bool) != Some(true) {
            return Err(bad(REQUEST_USAGE));
        }
        if self.is_on(name) {
            return Err(bad(ALREADY_ON).with_reason("already_on"));
        }
        if let Some(refused) =
            crate::switch_worker::refusal(&self.place(), self.consent.app_attached(), name)
        {
            return Err(refused);
        }
        let (reason, expires_in_ms, key) = options(args);
        let opening = Opening {
            parts: None,
            kind: SWITCH,
            target: name.to_string(),
            change: json!({"switch": name, "before": false, "after": true}),
            facts: json!([{"line": "consent-switch-on", "args": {"switch": name}}]),
            request: json!({"op": ops::SWITCH_REQUEST, "switch": name, "on": true}),
            reason,
            expires_in_ms,
            key,
        };
        if args.get("dryRun").and_then(Value::as_bool) == Some(true) {
            let would = self.consent.would(&opening)?;
            return Ok(
                json!({"dryRun": true, "op": ops::SWITCH_REQUEST, "would": "ask",
                "change": {"switch": name, "before": false, "after": true},
                "request": would, "error": null}),
            );
        }
        let opened = self.consent.open(caller, opening)?;
        let row = opened.row();
        Ok(json!({"state": row.state.as_str(), "request": self.consent.project(row, true)}))
    }

    fn off(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let name = name_of(args, OFF_USAGE)?;
        if !self.is_on(name) {
            return Ok(json!({"switch": name, "on": false, "changed": false}));
        }
        (self.turn)(name, false, &json!({"agentOff": true}))?;
        if let Some(inbox) = &self.inbox {
            let who = self.consent.who(caller);
            let session = who.session.as_deref().filter(|_| who.verified);
            let mut item = off_notice(name, who.tool.name(), session, now_ms());
            let headless = self.headless();
            if headless() {
                item.actions.clear();
                inbox.raise(item, None);
            } else {
                let responder: Arc<dyn Responder> = Arc::new(TurnOn {
                    name: name.to_string(),
                    turn: self.turn.clone(),
                    headless,
                });
                inbox.raise(item, Some(responder));
            }
        }
        Ok(json!({"switch": name, "on": false, "changed": true}))
    }

    pub fn apply(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
        run: &dyn Fn(&Value) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        ConsentOps::app_only(caller)?;
        let name = match op {
            ops::SETTINGS_SET_ACTING => "acting",
            ops::BRIDGE_SET => "bridge",
            _ => name_of(args, REQUEST_USAGE)?,
        };
        if args.get("on").and_then(Value::as_bool) != Some(true) {
            return Err(bad(ONLY_ON));
        }
        let id = args
            .get("consentId")
            .and_then(Value::as_str)
            .ok_or_else(|| bad(REQUEST_USAGE))?;
        let row = self.consent.use_for_app(caller, id, SWITCH, name)?;
        if self.is_on(name) {
            self.consent.withdraw_use(&row);
            return Err(bad(ALREADY_ON).with_reason("already_on"));
        }
        let result = run(args);
        let outcome = match &result {
            Ok(_) => Ok(json!({"switch": name, "on": true})),
            Err(e) => Err(e.message.clone()),
        };
        self.consent.finish_use(&row, outcome);
        result
    }
}

struct Recheck(Weak<SwitchAsks>);

impl Handler for Recheck {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) if asks.is_on(&row.target) => Err(ALREADY_ON.to_string()),
            _ => Ok(()),
        }
    }

    fn forwarded(&self, row: &Consent) -> bool {
        self.0.upgrade().is_some_and(|asks| {
            crate::switch_worker::forwarded(&asks.place(), asks.consent.app_attached(), &row.target)
        })
    }

    fn landed(&self, row: &Consent) -> Option<Value> {
        let asks = self.0.upgrade()?;
        asks.is_on(&row.target)
            .then(|| json!({"switch": row.target, "on": true}))
    }
}

impl OpGroup for SwitchAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::SWITCH_REQUEST | ops::SWITCH_OFF => Some(OpSpec::AGENT_ACTS),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SWITCH_REQUEST => self.request(caller, args),
            _ => self.off(caller, args),
        }
    }
}

#[cfg(test)]
#[path = "switch_asks_tests.rs"]
mod tests;
