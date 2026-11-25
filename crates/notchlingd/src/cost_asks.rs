use crate::cited::Cited;
use crate::consent_ops::{ConsentOps, Handler};
use crate::cost_profile_ops::CostProfileOps;
use crate::install_ops::parts::{EXPECT_AFTER, KEEP_GUARD};
use crate::op_group::OpGroup;
use crate::setup_cards::{fact, finish, forbidden, never, open, session_of, switch_off, take, Ask};
use crate::switches::{SwitchFlag, INSTALL_OFF};
use notchling_engine::consent::{Consent, COST_PROFILE};
use notchling_engine::cost_profile::model::tool_key;
use notchling_local_api::{ops, ApiError, Caller, OpSpec};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Weak};

const USAGE: &str = "cost.request takes {\"action\": \"apply\"|\"undo\"|\"profile\", \"args\": {...}, \"reason\": text?, \"expiresInMs\": number?, \"key\": text?, \"dryRun\": bool?}";
const NOTHING: &str = "There is nothing to change.";
const PROFILE_KEYS: [&str; 5] = ["preset", "tools", "picks", "habits", "reset"];

pub struct CostAsks {
    consent: Arc<ConsentOps>,
    cost: Arc<CostProfileOps>,
    switch: Arc<SwitchFlag>,
}

fn bad(text: &str) -> ApiError {
    ApiError::bad_request(text)
}

struct Switched(Weak<CostAsks>);

impl Handler for Switched {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) if row.change["action"] == "profile" || asks.switch.is_on() => Ok(()),
            Some(_) => Err(INSTALL_OFF.to_string()),
            None => Err("stopping".to_string()),
        }
    }
}

fn only_keys(inner: &Value, keys: &[&str]) -> Result<(), ApiError> {
    match inner.as_object() {
        Some(map) if map.keys().all(|k| keys.contains(&k.as_str())) => Ok(()),
        _ => Err(bad(USAGE)),
    }
}

impl CostAsks {
    pub fn new(
        consent: Arc<ConsentOps>,
        cost: Arc<CostProfileOps>,
        switch: Arc<SwitchFlag>,
    ) -> Arc<CostAsks> {
        let asks = Arc::new(CostAsks {
            consent: consent.clone(),
            cost,
            switch,
        });
        consent.register(COST_PROFILE, Arc::new(Switched(Arc::downgrade(&asks))));
        asks
    }

    fn gate(&self) -> Result<(), ApiError> {
        if self.switch.is_on() {
            Ok(())
        } else {
            Err(switch_off(INSTALL_OFF))
        }
    }

    fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| bad(USAGE))?;
        let inner = match args.get("args") {
            None | Some(Value::Null) => json!({}),
            Some(v) if v.is_object() => v.clone(),
            Some(_) => return Err(bad(USAGE)),
        };
        let who = self.consent.who(caller);
        session_of(&who)?;
        match action {
            "apply" => self.apply_card(caller, args, inner),
            "undo" => self.undo_card(caller, args, inner),
            "profile" => self.profile_card(caller, args, inner),
            _ => Err(bad(USAGE)),
        }
    }

    fn apply_card(&self, caller: &Caller, args: &Value, inner: Value) -> Result<Value, ApiError> {
        only_keys(&inner, &["consent", "wait"])?;
        self.gate()?;
        let opts = CostProfileOps::options_of(&inner)?;
        let parts = self.cost.apply_parts(&opts)?;
        if parts.is_empty() {
            return Err(bad(NOTHING).with_reason("nothing_to_do"));
        }
        let mut apply = Map::new();
        let mut names: Vec<&str> = opts.consent.iter().map(String::as_str).collect();
        names.sort_unstable();
        apply.insert("consent".into(), json!(names));
        if let Some(wait) = inner.get("wait").filter(|w| w.is_boolean()) {
            apply.insert("wait".into(), wait.clone());
        }
        let facts = json!([fact("consent-cost-apply", json!({"count": parts.len()}))]);
        self.ask(
            caller,
            args,
            "apply",
            &inner,
            "apply".to_string(),
            ops::COST_SETUP_APPLY,
            Value::Object(apply),
            facts,
            parts,
        )
    }

    fn undo_card(&self, caller: &Caller, args: &Value, inner: Value) -> Result<Value, ApiError> {
        only_keys(&inner, &["tool"])?;
        self.gate()?;
        let tool = CostProfileOps::tool_of(&inner)?;
        let parts = self.cost.undo_part_list(tool)?;
        if parts.is_empty() {
            return Err(bad(NOTHING).with_reason("nothing_to_do"));
        }
        let key = tool_key(tool);
        let facts = json!([fact(
            "consent-cost-undo",
            json!({"tool": key, "count": parts.len()})
        )]);
        self.ask(
            caller,
            args,
            "undo",
            &inner,
            format!("undo:{key}"),
            ops::COST_SETUP_UNDO,
            json!({"tool": key}),
            facts,
            parts,
        )
    }

    fn profile_card(&self, caller: &Caller, args: &Value, inner: Value) -> Result<Value, ApiError> {
        if inner
            .as_object()
            .is_some_and(|m| m.keys().any(|k| k.to_lowercase().contains("guard")))
        {
            return Err(never());
        }
        only_keys(&inner, &PROFILE_KEYS)?;
        let part = self.cost.profile_part(&inner)?;
        let mut apply = inner.as_object().cloned().unwrap_or_default();
        apply.insert("expect".into(), part["before"].clone());
        let facts = json!([fact(
            "consent-cost-profile",
            json!({"count": part["fields"].as_array().map_or(0, Vec::len)})
        )]);
        self.ask(
            caller,
            args,
            "profile",
            &inner,
            "profile".to_string(),
            ops::COST_PROFILE_SET,
            Value::Object(apply),
            facts,
            vec![part],
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn ask(
        &self,
        caller: &Caller,
        args: &Value,
        action: &str,
        inner: &Value,
        target: String,
        op: &str,
        apply: Value,
        facts: Value,
        parts: Vec<Value>,
    ) -> Result<Value, ApiError> {
        open(
            &self.consent,
            caller,
            args,
            Ask {
                kind: COST_PROFILE,
                op: ops::COST_REQUEST,
                action,
                inner,
                target,
                change: json!({"action": action, "apply": {"op": op, "args": apply}}),
                facts,
                parts,
            },
        )
    }
}

impl Cited for CostAsks {
    fn claims(&self, op: &str, args: &Value) -> bool {
        [
            ops::COST_SETUP_APPLY,
            ops::COST_SETUP_UNDO,
            ops::COST_PROFILE_SET,
        ]
        .contains(&op)
            && args.get("consentId").is_some()
    }

    fn apply(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
        run: &dyn Fn(&Value) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        let taken = take(&self.consent, caller, COST_PROFILE, op, args)?;
        let mut given = taken.args.clone();
        given.insert(KEEP_GUARD.into(), Value::Bool(true));
        if op == ops::COST_PROFILE_SET {
            if !taken.only.contains_key("profile") {
                self.consent.withdraw_use(&taken.row);
                return Err(forbidden(
                    "That part was not allowed on the card.",
                    "needs_consent",
                ));
            }
        } else {
            if !self.switch.is_on() {
                self.consent.withdraw_use(&taken.row);
                return Err(forbidden(INSTALL_OFF, "switch_off"));
            }
            given.insert("parts".into(), Value::Object(taken.only.clone()));
            if op == ops::COST_SETUP_APPLY {
                given.insert(EXPECT_AFTER.into(), Value::Object(taken.after.clone()));
            }
        }
        let outcome = run(&Value::Object(given));
        finish(&self.consent, &taken, outcome)
    }
}

impl OpGroup for CostAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::COST_REQUEST).then_some(OpSpec::AGENT_ACTS)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::COST_REQUEST => self.request(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[cfg(test)]
#[path = "cost_asks_tests.rs"]
mod tests;
