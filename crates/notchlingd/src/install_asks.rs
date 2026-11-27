use crate::agy_sidecar_ops::part::{Config, Sidecar, PART as SIDECAR};
use crate::cited::Cited;
use crate::consent_ops::{ConsentOps, Handler};
use crate::install_ops::parts::{EXPECT_AFTER, KEEP_GUARD};
use crate::install_ops::InstallOps;
use crate::op_group::OpGroup;
use crate::setup_cards::{fact, finish, forbidden, never, open, session_of, switch_off, take, Ask};
use crate::switches::{SwitchFlag, INSTALL_OFF};
use notchling_engine::consent::{Consent, INSTALL};
use notchling_local_api::{ops, ApiError, Caller, OpSpec};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

const USAGE: &str = "install.request takes {\"parts\": [names]?, \"reason\": text?, \"expiresInMs\": number?, \"key\": text?, \"dryRun\": bool?}";
const SIDECAR_ALONE: &str = "The sidecar is its own request. Ask for it by itself.";
const NOTHING: &str = "There is nothing to set up.";

pub struct InstallAsks {
    consent: Arc<ConsentOps>,
    install: Arc<InstallOps>,
    sidecar: Arc<dyn Sidecar>,
    switch: Arc<SwitchFlag>,
}

fn bad(text: &str) -> ApiError {
    ApiError::bad_request(text)
}

struct Switched(Weak<InstallAsks>);

impl Handler for Switched {
    fn recheck(&self, _row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) if asks.switch.is_on() => Ok(()),
            Some(_) => Err(INSTALL_OFF.to_string()),
            None => Err("stopping".to_string()),
        }
    }
}

impl InstallAsks {
    pub fn new(
        consent: Arc<ConsentOps>,
        install: Arc<InstallOps>,
        sidecar: Arc<dyn Sidecar>,
        switch: Arc<SwitchFlag>,
    ) -> Arc<InstallAsks> {
        let asks = Arc::new(InstallAsks {
            consent: consent.clone(),
            install,
            sidecar,
            switch,
        });
        consent.register(INSTALL, Arc::new(Switched(Arc::downgrade(&asks))));
        asks
    }

    fn names(args: &Value) -> Result<Option<Vec<String>>, ApiError> {
        match args.get("parts") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|i| i.as_str().map(str::to_string).ok_or_else(|| bad(USAGE)))
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(bad(USAGE)),
        }
    }

    fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let who = self.consent.who(caller);
        session_of(&who)?;
        let names = Self::names(args)?;
        if !self.switch.is_on() {
            return Err(switch_off(INSTALL_OFF));
        }
        match names.as_deref() {
            Some(list) if list.iter().any(|n| n == SIDECAR) => {
                if list.len() != 1 {
                    return Err(bad(SIDECAR_ALONE));
                }
                self.sidecar_card(caller, args)
            }
            _ => self.files_card(caller, args, names),
        }
    }

    fn files_card(
        &self,
        caller: &Caller,
        args: &Value,
        names: Option<Vec<String>>,
    ) -> Result<Value, ApiError> {
        let what = names.map(|n| json!(n)).unwrap_or(Value::Null);
        let plan = self.install.agent_plan(&what)?;
        if plan.removes {
            return Err(never());
        }
        if let Some(why) = plan.blocked {
            return Err(bad(&why).with_reason("blocked"));
        }
        if plan.parts.is_empty() {
            return Err(bad(NOTHING).with_reason("nothing_to_do"));
        }
        let apply_args = if what.is_null() {
            json!({})
        } else {
            json!({"what": what})
        };
        let facts = json!([fact(
            "consent-install-files",
            json!({"count": plan.parts.len()})
        )]);
        open(
            &self.consent,
            caller,
            args,
            Ask {
                kind: INSTALL,
                op: ops::INSTALL_REQUEST,
                action: "set_up",
                inner: &what,
                target: "set-up".to_string(),
                change: json!({"action": "set_up", "what": what,
                    "apply": {"op": ops::INSTALL_APPLY, "args": apply_args}}),
                facts,
                parts: plan.parts,
            },
        )
    }

    fn sidecar_card(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let part = match self.sidecar.config_part() {
            Config::Part(part) => part,
            Config::Already => {
                return Err(bad("The sidecar is already on.").with_reason("nothing_to_do"))
            }
            Config::Blocked(why) => return Err(bad(&why).with_reason("blocked")),
        };
        let apply = json!({"action": "turn_on", "confirm": true});
        open(
            &self.consent,
            caller,
            args,
            Ask {
                kind: INSTALL,
                op: ops::INSTALL_REQUEST,
                action: "sidecar",
                inner: &json!([SIDECAR]),
                target: SIDECAR.to_string(),
                change: json!({"action": "sidecar",
                    "apply": {"op": ops::AGY_SIDECAR_SETUP, "args": apply}}),
                facts: json!([fact("consent-install-sidecar", json!({}))]),
                parts: vec![part],
            },
        )
    }
}

impl Cited for InstallAsks {
    fn claims(&self, op: &str, args: &Value) -> bool {
        (op == ops::INSTALL_APPLY || op == ops::AGY_SIDECAR_SETUP)
            && args.get("consentId").is_some()
    }

    fn apply(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
        run: &dyn Fn(&Value) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        let taken = take(&self.consent, caller, INSTALL, op, args)?;
        if !self.switch.is_on() {
            self.consent.withdraw_use(&taken.row);
            return Err(forbidden(INSTALL_OFF, "switch_off"));
        }
        let mut given = taken.args.clone();
        given.insert("parts".into(), Value::Object(taken.only.clone()));
        if op == ops::INSTALL_APPLY {
            given.insert(EXPECT_AFTER.into(), Value::Object(taken.after.clone()));
            given.insert(KEEP_GUARD.into(), Value::Bool(true));
        }
        let outcome = run(&Value::Object(given));
        finish(&self.consent, &taken, outcome)
    }
}

impl OpGroup for InstallAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::INSTALL_REQUEST).then_some(OpSpec::AGENT_ACTS)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::INSTALL_REQUEST => self.request(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[cfg(test)]
#[path = "install_asks_tests.rs"]
mod tests;
