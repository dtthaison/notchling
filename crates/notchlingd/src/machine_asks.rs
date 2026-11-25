use super::machine_asks_plans::{need_word, ACTIONS, USAGE};
use super::machines_ops::{bad, caller_of, forbidden, never_at, Machines};
use crate::cited::Cited;
use crate::consent_ops::{options, ConsentOps, Handler, Opening};
use crate::op_group::OpGroup;
use notchling_engine::accounts::Secret;
use notchling_engine::bridge::net::crypto::sha256_hex;
use notchling_engine::consent::{Consent, MACHINE};
use notchling_engine::machines::agent_plan::digest;
use notchling_engine::machines::claims::HELD;
use notchling_engine::machines::engine_plan::AdminAnswer;
use notchling_engine::machines::names::same_name;
use notchling_engine::organizations::PERSONAL_ID;
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

pub const USER_ONLY: [(&str, &str); 5] = [
    ("trust", "host_key"),
    ("trust_host", "host_key"),
    ("login", "vault"),
    ("token", "vault"),
    ("join", "cluster"),
];
pub const RUNS: [&str; 3] = ["fix", "setup", "running"];

fn dry_run(args: &Value) -> bool {
    args.get("dryRun").and_then(Value::as_bool) == Some(true)
}

pub fn scrub_facts(facts: &Value, mine: &str) -> Value {
    let Some(list) = facts.as_array() else {
        return facts.clone();
    };
    let out: Vec<Value> = list
        .iter()
        .map(|fact| {
            if fact["line"] != json!("consent-network-drops") {
                return fact.clone();
            }
            let mut own = 0usize;
            let mut other = 0usize;
            for g in fact["args"]["groups"].as_array().into_iter().flatten() {
                let n = match g["machines"].as_array() {
                    Some(names) => names.len(),
                    None => g["count"].as_u64().unwrap_or(0) as usize,
                };
                if g["organization"].as_str() == Some(mine) {
                    own += n;
                } else {
                    other += n;
                }
            }
            let mut groups = Vec::new();
            if own > 0 {
                groups.push(json!({"organization": mine, "count": own}));
            }
            if other > 0 {
                groups.push(json!({"organization": "other", "count": other}));
            }
            json!({"line": fact["line"], "args": {"count": own + other, "groups": groups}})
        })
        .collect();
    Value::Array(out)
}

fn mine_of(row: &Consent) -> String {
    row.organization
        .clone()
        .unwrap_or_else(|| PERSONAL_ID.to_string())
}

pub struct MachineAsks {
    me: Weak<MachineAsks>,
    pub machines: Arc<Machines>,
    consent: Arc<ConsentOps>,
}

impl MachineAsks {
    pub fn new(machines: Arc<Machines>) -> Arc<MachineAsks> {
        let consent = machines.consent.clone();
        let asks = Arc::new_cyclic(|me| MachineAsks {
            me: me.clone(),
            machines,
            consent: consent.clone(),
        });
        consent.register(MACHINE, Arc::new(Hooks(asks.me.clone())));
        asks
    }

    fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let who = self.consent.who(caller);
        let (_, mine) = caller_of(&who)?;
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| bad(USAGE))?;
        if let Some((_, family)) = USER_ONLY.iter().find(|(a, _)| *a == action) {
            return Err(never_at(family));
        }
        if !ACTIONS.contains(&action) {
            return Err(bad(USAGE));
        }
        self.machines.gate()?;
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
        let request = json!({"op": ops::MACHINES_REQUEST, "action": action, "digest": digest});
        if !dry_run(args) {
            if let Some(first) = self.earlier(caller, key.as_deref(), &request)? {
                return Ok(self.shown(&first));
            }
        }
        if !dry_run(args) {
            self.machines.name_refusal(action, &inner)?;
        }
        let plan = self.machines.plan(action, &inner, &mine)?;
        let opening = Opening {
            parts: None,
            kind: MACHINE,
            target: plan.target.clone(),
            request,
            change: plan.change,
            facts: plan.facts,
            reason,
            expires_in_ms,
            key,
        };
        if dry_run(args) {
            let mut would = self.consent.would(&opening)?;
            would["facts"] = scrub_facts(&opening.facts, &mine);
            let change = self.machines.agent_change(&opening.change);
            return Ok(json!({"dryRun": true, "op": ops::MACHINES_REQUEST,
                "would": "ask", "change": change, "request": would, "error": null}));
        }
        let opened = self.consent.open(caller, opening)?;
        Ok(self.shown(opened.row()))
    }

    fn shown(&self, row: &Consent) -> Value {
        json!({"state": row.state.as_str(), "request": self.consent.project(row, true)})
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
        match self.consent.earlier(caller, MACHINE, key)? {
            Some(first) if first.request == *request => Ok(Some(first)),
            Some(_) => Err(
                bad("That key was used for a different request. Use a new key.")
                    .with_reason("key_reused"),
            ),
            None => Ok(None),
        }
    }

    pub fn still_fits(&self, row: &Consent) -> Result<(), String> {
        if !(self.machines.on)() {
            return Err("Machines is off".to_string());
        }
        let action = row.change["action"].as_str().unwrap_or_default();
        let args = row.change.get("args").cloned().unwrap_or(Value::Null);
        let now = self
            .machines
            .plan(action, &args, &mine_of(row))
            .map_err(|e| e.message.clone())?;
        if now.change == row.change && now.facts == row.facts {
            Ok(())
        } else {
            Err("the machines changed since the request".to_string())
        }
    }

    fn answer(row: &Consent, admin: &Value) -> Result<AdminAnswer, ApiError> {
        match row.change["admin"].as_str() {
            Some("password") => {
                let typed = admin
                    .get("password")
                    .and_then(Value::as_str)
                    .filter(|p| !p.is_empty());
                let entry = admin
                    .get("vault")
                    .and_then(Value::as_str)
                    .filter(|e| !e.is_empty());
                match (typed, entry) {
                    (Some(p), _) => Ok(AdminAnswer::Password(Secret::new(p))),
                    (None, Some(e)) => Ok(AdminAnswer::Vault(e.to_string())),
                    _ => Err(bad("This step needs the machine's admin password.")
                        .with_reason("admin_needed")),
                }
            }
            Some("root") => Ok(AdminAnswer::RootConfirmed),
            _ => Ok(AdminAnswer::None),
        }
    }

    fn perform(&self, row: &Consent, admin: &Value) -> Result<Value, ApiError> {
        let mine = mine_of(row);
        let args = &row.change["args"];
        let action = row.change["action"].as_str().unwrap_or_default();
        let m = &self.machines;
        match action {
            "add" => {
                let machine = m.added_machine(args, &mine)?;
                let name = machine.name.clone();
                m.commit(|list| {
                    if list.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
                        return Err(forbidden("That name is taken now.", "stale"));
                    }
                    list.push(machine);
                    Ok(true)
                })?;
                Ok(json!({"added": [name]}))
            }
            "import" => {
                let made = m.imported(args, &mine)?;
                let names: Vec<String> = made.iter().map(|x| x.name.clone()).collect();
                m.commit(|list| {
                    for x in &made {
                        if list.iter().any(|y| y.name.eq_ignore_ascii_case(&x.name)) {
                            return Err(forbidden("That name is taken now.", "stale"));
                        }
                    }
                    list.extend(made);
                    Ok(true)
                })?;
                Ok(json!({"added": names}))
            }
            "addresses" => {
                let (machine, list) = m.typed_list(args, &mine)?;
                m.commit(|all| {
                    let Some(found) = all.iter_mut().find(|x| x.name == machine.name) else {
                        return Err(forbidden("That machine is gone.", "stale"));
                    };
                    found.set_addresses(list);
                    Ok(true)
                })?;
                Ok(json!({"machine": machine.name}))
            }
            "remove" => {
                let name = args["machine"].as_str().unwrap_or_default();
                let machine = m.owned(name, &mine, super::machines_ops::never)?;
                m.commit(|all| {
                    let before = all.len();
                    all.retain(|x| x.name != machine.name);
                    Ok(all.len() != before)
                })?;
                m.engine.forget(&machine.name);
                Ok(json!({"removed": machine.name}))
            }
            "fix" | "setup" | "running" => self.run_plan(row, action, &mine, admin),
            _ => Err(bad(USAGE)),
        }
    }

    fn run_plan(
        &self,
        row: &Consent,
        action: &str,
        mine: &str,
        admin: &Value,
    ) -> Result<Value, ApiError> {
        let m = &self.machines;
        let args = &row.change["args"];
        let name = args["machine"].as_str().unwrap_or_default();
        let machine = m.owned(name, mine, super::machines_ops::never)?;
        let kind = m.run_kind(action, args, &machine)?;
        let shown = m.shown_plan(&machine, &kind)?;
        let need = need_word(&m.engine.admin_need_of(&shown));
        if row.change["digest"] != json!(digest(&shown)) || row.change["admin"] != json!(need) {
            m.engine.cancel(shown.id);
            return Err(forbidden("The steps changed since the request.", "stale"));
        }
        let answer = match Self::answer(row, admin) {
            Ok(a) => a,
            Err(e) => {
                m.engine.cancel(shown.id);
                return Err(e);
            }
        };
        let confirmed = match m.engine.confirm(shown.id, answer) {
            Ok(c) => c,
            Err(why) => {
                m.engine.cancel(shown.id);
                return Err(bad(&why).with_reason("refused"));
            }
        };
        let outcome = m.engine.run(confirmed);
        if outcome.ok {
            Ok(json!({"machine": machine.name, "ran": shown.steps.len()}))
        } else if outcome.message.contains(HELD) {
            Err(forbidden(HELD, "busy"))
        } else {
            Err(ApiError::new(ErrorCode::Failed, "The step didn't finish.")
                .with_reason("step_failed"))
        }
    }

    pub fn applying(
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
        let admin = given
            .as_object_mut()
            .and_then(|map| map.remove("admin"))
            .unwrap_or(Value::Null);
        if let Some(map) = given.as_object_mut() {
            map.remove("consentId");
        }
        let stored = self
            .consent
            .get_row(id)
            .map_err(|_| forbidden("That request is gone.", "needs_consent"))?;
        if stored.kind != MACHINE || stored.change["apply"] != json!({"op": op, "args": given}) {
            return Err(forbidden(
                "That request is for something else.",
                "needs_consent",
            ));
        }
        let runs = RUNS.contains(&stored.change["action"].as_str().unwrap_or_default());
        if runs {
            self.saved_entry(&stored, &admin)?;
        }
        let row = self
            .consent
            .take_for_app(caller, id, MACHINE, Some(&stored.target))?;
        if let Err(why) = self.still_fits(&row) {
            self.consent.withdraw_use(&row);
            return Err(forbidden(&format!("It no longer fits: {why}."), "stale"));
        }
        let outcome = if op == ops::MACHINES_APPLY {
            self.perform(&row, &admin)
        } else {
            run(&given)
        };
        match outcome {
            Ok(result) => {
                let action = row.change["action"].clone();
                self.consent.finish_use(
                    &row,
                    Ok(json!({"action": action, "done": true, "result": result})),
                );
                Ok(result)
            }
            Err(e) => {
                if reason_of(&e) == Some("stale") {
                    self.consent.withdraw_use(&row);
                } else {
                    self.consent.finish_use(&row, Err(e.message.clone()));
                }
                Err(e)
            }
        }
    }

    fn saved_entry(&self, row: &Consent, admin: &Value) -> Result<(), ApiError> {
        let answer = Self::answer(row, admin)?;
        let AdminAnswer::Vault(entry) = answer else {
            return Ok(());
        };
        let name = row.change["machine"]
            .as_str()
            .or_else(|| row.change["args"]["machine"].as_str())
            .unwrap_or_default();
        let saved = self
            .machines
            .all()
            .into_iter()
            .find(|m| same_name(&m.name, name))
            .and_then(|m| m.admin_secret().map(str::to_owned));
        if saved.as_deref().is_some_and(|s| same_name(s, &entry)) {
            Ok(())
        } else {
            Err(
                bad("That isn't the Vault entry saved for this machine's admin password.")
                    .with_reason("vault_entry"),
            )
        }
    }
}

fn reason_of(e: &ApiError) -> Option<&str> {
    e.data.as_ref()?.get("reason")?.as_str()
}

impl Cited for MachineAsks {
    fn claims(&self, op: &str, args: &Value) -> bool {
        (op == ops::MACHINES_APPLY || op == ops::TAILSCALE_SWITCH)
            && args.get("consentId").is_some()
    }

    fn apply(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
        run: &dyn Fn(&Value) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        self.applying(caller, op, args, run)
    }
}

struct Hooks(Weak<MachineAsks>);

impl Handler for Hooks {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) => asks.still_fits(row),
            None => Err("stopping".to_string()),
        }
    }

    fn shape(&self, row: &Consent, view: &mut Value, agent: bool) {
        if agent {
            view["facts"] = scrub_facts(&row.facts, &mine_of(row));
            if let Some(change) = view.get_mut("change").and_then(Value::as_object_mut) {
                change.remove("adminEntry");
            }
        }
    }

    fn handover(&self, _row: &Consent, _who: &AgentIdentity) -> Option<Value> {
        None
    }
}

impl OpGroup for MachineAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::MACHINES_REQUEST => Some(OpSpec::AGENT_ACTS),
            ops::MACHINES_APPLY => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::MACHINES_REQUEST => self.request(caller, args),
            ops::MACHINES_APPLY => Err(forbidden(
                "That needs a request the user has allowed.",
                "needs_consent",
            )),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}
