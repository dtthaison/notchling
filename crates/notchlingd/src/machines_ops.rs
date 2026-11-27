use crate::cluster_asks::Admin;
use crate::consent_ops::ConsentOps;
use crate::op_group::OpGroup;
use notchling_engine::machines::agent_plan::fix_refusal;
use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::registry::{write_lock, Machine, RegistryFiles, RegistryState};
use notchling_engine::machines::ssh_key::file_name;
use notchling_engine::machines::step::{CheckState, StepId};
use notchling_engine::machines::tailscale::{available, Network, Tailscale};
use notchling_engine::organizations::{Organizations, PERSONAL_ID};
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::Arc;

pub const NO_SESSION: &str = "Only an agent inside a Notchling session may do that.";
pub const NEVER: &str = "Only you can do this, in Notchling.";
pub const OTHER_ORG: &str = "That machine is in another organization.";
pub const UNKNOWN_OWNER: &str = "Notchling can't tell which organization that machine is in, so only the user can change it, in the app.";
pub const MACHINES_OFF: &str = "Machines is off, so no agent may reach or change a machine.";

pub type SwitchOn = Arc<dyn Fn() -> bool + Send + Sync>;
pub type Changed = Arc<dyn Fn() + Send + Sync>;

pub struct Plan {
    pub action: &'static str,
    pub target: String,
    pub change: Value,
    pub facts: Value,
}

pub struct Machines {
    pub engine: Arc<SetupEngine>,
    pub registry: Arc<RegistryFiles>,
    pub orgs: Organizations,
    pub tailscale: Tailscale,
    pub members: Arc<dyn Admin>,
    pub on: SwitchOn,
    pub consent: Arc<ConsentOps>,
    pub changed: Changed,
}

pub fn bad(text: &str) -> ApiError {
    ApiError::bad_request(text)
}

pub fn forbidden(text: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, text).with_reason(reason)
}

pub fn never() -> ApiError {
    forbidden(NEVER, "never").with_field("never", json!(true))
}

pub fn never_at(family: &str) -> ApiError {
    never().with_field("pointsAt", json!(family))
}

pub fn machines_off() -> ApiError {
    forbidden(MACHINES_OFF, "switch_off")
        .with_field("switch", json!("machines"))
        .with_field(
            "ask",
            json!({"command": "notchling switch on machines", "kind": "switch"}),
        )
}

pub fn caller_of(who: &AgentIdentity) -> Result<(String, String), ApiError> {
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

pub fn mask_email(text: &str) -> String {
    match text.split_once('@') {
        Some((user, host)) => {
            let first: String = user.chars().take(1).collect();
            format!("{first}***@{host}")
        }
        None => text.to_string(),
    }
}

pub fn network_view(n: &Network) -> Value {
    json!({
        "id": n.id,
        "tailnet": mask_email(&n.tailnet),
        "account": mask_email(&n.account),
        "active": n.active,
    })
}

pub fn address_line(m: &Machine) -> String {
    m.address_list()
        .iter()
        .map(|a| format!("{}@{}", a.user.as_deref().unwrap_or(&m.user), a.host))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Machines {
    pub fn gate(&self) -> Result<(), ApiError> {
        if (self.on)() {
            Ok(())
        } else {
            Err(machines_off())
        }
    }

    pub fn mine(&self, caller: &Caller) -> Result<String, ApiError> {
        let who = self.consent.who(caller);
        caller_of(&who).map(|(_, org)| org)
    }

    pub fn all(&self) -> Vec<Machine> {
        match self.registry.load() {
            RegistryState::Loaded { machines, .. } => machines,
            _ => Vec::new(),
        }
    }

    pub fn commit(
        &self,
        change: impl FnOnce(&mut Vec<Machine>) -> Result<bool, ApiError>,
    ) -> Result<(), ApiError> {
        let _writing = write_lock();
        let mut list = match self.registry.load() {
            RegistryState::Loaded { machines, .. } => machines,
            RegistryState::Missing => Vec::new(),
            RegistryState::Unreadable(_) => {
                return Err(bad("The machine list can't be read.").with_reason("unreadable"))
            }
        };
        if !change(&mut list)? {
            return Ok(());
        }
        if self.registry.save(&list) {
            (self.changed)();
            Ok(())
        } else {
            Err(ApiError::new(
                ErrorCode::Failed,
                "The machine list couldn't be saved.",
            ))
        }
    }

    pub fn owned(
        &self,
        name: &str,
        mine: &str,
        deny: fn() -> ApiError,
    ) -> Result<Machine, ApiError> {
        let all = self.all();
        let mut found = all.iter().filter(|m| m.name.eq_ignore_ascii_case(name));
        let (one, two) = (found.next(), found.next());
        let Some(machine) = one else {
            return Err(bad("No such machine.").with_reason("no_machine"));
        };
        if two.is_some() || self.orgs.view().is_closed() {
            return Err(forbidden(UNKNOWN_OWNER, "unknown_owner"));
        }
        if machine.organization_id() != mine {
            return Err(deny());
        }
        Ok(machine.clone())
    }

    pub fn machine_json(&self, m: &Machine) -> Value {
        let results = self.engine.results(&m.name);
        let count = |f: fn(&CheckState) -> bool| results.values().filter(|r| f(&r.state)).count();
        json!({
            "name": m.name,
            "host": m.host,
            "user": m.user,
            "os": m.os.map(|o| format!("{o:?}").to_ascii_lowercase()),
            "organization": m.organization_id(),
            "addresses": m.address_list().iter().map(|a| json!({
                "kind": a.kind.key(), "host": a.host, "tailnet": a.tailnet_name(),
            })).collect::<Vec<_>>(),
            "sshKey": m.ssh_key().map(file_name),
            "role": m.node.as_ref().map(|n| if n.choices.can_be_master { "master" } else { "worker" }),
            "checked": {
                "pass": count(CheckState::is_pass),
                "fail": count(CheckState::is_fail),
            },
        })
    }

    pub fn list(&self, caller: &Caller) -> Result<Value, ApiError> {
        self.gate()?;
        let mine = self.mine(caller)?;
        let machines: Vec<Value> = self
            .all()
            .iter()
            .filter(|m| m.organization_id() == mine)
            .map(|m| self.machine_json(m))
            .collect();
        Ok(json!({"machines": machines}))
    }

    pub fn check(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        self.gate()?;
        let mine = self.mine(caller)?;
        let asked = args.get("machine").and_then(Value::as_str).map(str::trim);
        let targets: Vec<Machine> = match asked.filter(|s| !s.is_empty()) {
            Some(name) => {
                vec![self.owned(name, &mine, || forbidden(OTHER_ORG, "other_organization"))?]
            }
            None => self
                .all()
                .into_iter()
                .filter(|m| m.organization_id() == mine)
                .collect(),
        };
        let names: Vec<String> = targets.iter().map(|m| m.name.clone()).collect();
        self.engine.refresh_cheap(Some(&names));
        let out: Vec<Value> = targets.iter().map(|m| self.checked(m)).collect();
        Ok(json!({"machines": out}))
    }

    fn checked(&self, m: &Machine) -> Value {
        let results = self.engine.results(&m.name);
        let steps: Vec<Value> = self
            .engine
            .steps(m)
            .iter()
            .filter_map(|s| {
                let r = results.get(&s.id)?;
                let (state, text) = match &r.state {
                    CheckState::Pass(t) => ("pass", t),
                    CheckState::Fail(t) => ("fail", t),
                    CheckState::CantTell(t) => ("cant_tell", t),
                };
                let mut row =
                    json!({"id": s.id.raw(), "title": s.title, "state": state, "text": text});
                let display = s.plan().map(|p| p.display.as_str()).unwrap_or_default();
                if let Some(family) = fix_refusal(&s.id, display) {
                    row["agentMayFix"] = json!(false);
                    row["pointsAt"] = json!(family);
                }
                Some(row)
            })
            .collect();
        let unknown_host = results
            .get(&StepId::REACHABLE)
            .is_some_and(|r| is_unknown_host(r.state.text()));
        let mut out = self.machine_json(m);
        out["steps"] = json!(steps);
        if unknown_host {
            out["hostKey"] = json!("unknown: only the user can trust a new host key, in the app");
        }
        out
    }

    pub fn peers(&self, caller: &Caller) -> Result<Value, ApiError> {
        self.gate()?;
        self.mine(caller)?;
        let status = self.tailscale.status().map_err(ApiError::internal)?;
        let all = self.all();
        let peers: Vec<Value> = available(&status, &all)
            .iter()
            .map(|p| {
                json!({
                    "name": p.short(), "host": p.host(), "os": p.os,
                    "ips": p.ips, "online": p.online,
                })
            })
            .collect();
        Ok(json!({"tailnet": mask_email(&status.tailnet), "peers": peers}))
    }

    pub fn networks(&self, caller: &Caller) -> Result<Value, ApiError> {
        self.gate()?;
        self.mine(caller)?;
        let list = self.tailscale.networks().map_err(ApiError::internal)?;
        Ok(json!({"networks": list.iter().map(network_view).collect::<Vec<_>>()}))
    }
}

pub fn is_unknown_host(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("host key verification failed") || lower.contains("remote host identification")
}

pub struct Reads(pub Arc<Machines>);

impl OpGroup for Reads {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        matches!(
            op,
            ops::MACHINES_LIST
                | ops::MACHINES_CHECK
                | ops::MACHINES_PLAN
                | ops::MACHINES_PEERS
                | ops::NETWORK_LIST
        )
        .then_some(OpSpec::AGENT_READ)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::MACHINES_LIST => self.0.list(caller),
            ops::MACHINES_CHECK => self.0.check(caller, args),
            ops::MACHINES_PLAN => self.0.plan_read(caller, args),
            ops::MACHINES_PEERS => self.0.peers(caller),
            ops::NETWORK_LIST => self.0.networks(caller),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[notchling_local_api::topics::MACHINES]
    }
}
