use super::machines_ops::{
    address_line, bad, forbidden, mask_email, never, never_at, Machines, Plan,
};
use notchling_engine::machines::address;
use notchling_engine::machines::addresses::{
    infer_kind, list_problems, AddressKind, MachineAddress,
};
use notchling_engine::machines::agent_plan::{digest, kind_refusal, refusal};
use notchling_engine::machines::engine_plan::{AdminNeed, PlanKind, ShownPlan};
use notchling_engine::machines::forms::machine_problems;
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::ssh_key::file_name;
use notchling_engine::machines::step::StepId;
use notchling_engine::machines::tailscale::{import, Network, Picked};
use notchling_engine::naming;
use notchling_engine::organizations::PERSONAL_ID;
use notchling_local_api::{ops, ApiError, Caller, ErrorCode};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const ACTIONS: [&str; 11] = [
    "add",
    "import",
    "addresses",
    "remove",
    "fix",
    "setup",
    "running",
    "install",
    "update",
    "network",
    "rename",
];
pub const USAGE: &str = "machines.request takes {\"action\": \"add\"|\"import\"|\"addresses\"|\"remove\"|\"fix\"|\"setup\"|\"running\"|\"install\"|\"update\"|\"network\", \"args\": {...}}";

fn apply_of() -> Value {
    json!({"op": ops::MACHINES_APPLY, "args": {}})
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn known_keys(args: &Value, keys: &[&str]) -> Result<(), ApiError> {
    match args.as_object() {
        Some(map) if map.keys().all(|k| keys.contains(&k.as_str())) => Ok(()),
        _ => Err(bad(USAGE)),
    }
}

pub fn need_word(need: &AdminNeed) -> &'static str {
    match need {
        AdminNeed::None => "none",
        AdminNeed::Password => "password",
        AdminNeed::RootConfirm => "root",
        AdminNeed::Refused(_) => "refused",
    }
}

pub fn organization_value(mine: &str) -> Option<String> {
    (mine != PERSONAL_ID).then(|| mine.to_string())
}

impl Machines {
    pub fn machine_fact(&self, m: &Machine) -> Value {
        json!({"line": "consent-machine", "args": {
            "machine": m.name, "host": m.host, "user": m.user,
            "organization": m.organization_id(), "addresses": address_line(m),
            "key": m.ssh_key().map(file_name).unwrap_or_default(),
        }})
    }

    pub fn plan_read(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        self.gate()?;
        let mine = self.mine(caller)?;
        let (action, inner) = match (text(args, "action"), text(args, "machine")) {
            (Some(action), _) => (
                action.to_string(),
                args.get("args").cloned().unwrap_or_else(|| json!({})),
            ),
            (None, Some(machine)) => ("setup".to_string(), json!({"machine": machine})),
            _ => return Err(bad(USAGE)),
        };
        let plan = self.plan(&action, &inner, &mine)?;
        Ok(
            json!({"action": plan.action, "target": plan.target, "facts": super::machine_asks::scrub_facts(&plan.facts, &mine), "change": self.agent_change(&plan.change)}),
        )
    }

    pub fn agent_change(&self, change: &Value) -> Value {
        let mut out = change.clone();
        if let Some(map) = out.as_object_mut() {
            map.remove("apply");
            map.remove("adminEntry");
        }
        out
    }

    pub fn plan(&self, action: &str, args: &Value, mine: &str) -> Result<Plan, ApiError> {
        match action {
            "add" => self.plan_add(args, mine),
            "import" => self.plan_import(args, mine),
            "addresses" => self.plan_addresses(args, mine),
            "remove" => self.plan_remove(args, mine),
            "fix" => self.plan_run("fix", args, mine),
            "setup" => self.plan_run("setup", args, mine),
            "running" => self.plan_run("running", args, mine),
            "network" => self.plan_network(args),
            "install" | "update" | "rename" => Err(ApiError::new(
                ErrorCode::Unimplemented,
                format!("machines {action} is not built yet"),
            )
            .with_reason("not_yet")),
            _ => Err(bad(USAGE)),
        }
    }

    pub fn added_machine(&self, args: &Value, mine: &str) -> Result<Machine, ApiError> {
        known_keys(args, &["address", "name", "sshKey"])?;
        let typed = text(args, "address").ok_or_else(|| bad(USAGE))?;
        if !typed.contains('@') {
            return Err(bad("Type the address as user@host.").with_reason("bad_value"));
        }
        let existing: Vec<String> = self.all().iter().map(|m| m.name.clone()).collect();
        let parsed = address::parse(typed, "", &existing)
            .map_err(|why| bad(&why).with_reason("bad_value"))?;
        let name = text(args, "name").unwrap_or(&parsed.name).to_string();
        if let Some(why) = machine_problems(&name, &parsed.host, &parsed.user, &existing)
            .into_iter()
            .next()
        {
            return Err(bad(&why).with_reason("bad_value"));
        }
        let mut machine = Machine::new(&name, &parsed.host, &parsed.user);
        machine.organization = organization_value(mine);
        if let Some(key) = text(args, "sshKey") {
            if !notchling_engine::machines::ssh_key::shape_ok(key) {
                return Err(bad("That isn't a key file path.").with_reason("bad_value"));
            }
            machine.set_ssh_key(Some(key));
        }
        Ok(machine)
    }

    pub fn name_refusal(&self, action: &str, args: &Value) -> Result<(), ApiError> {
        if action != "add" {
            return Ok(());
        }
        let Some(name) = text(args, "name") else {
            return Ok(());
        };
        let known = self.all().iter().any(|m| m.name == name);
        if !known && naming::machine_generic(name) {
            return Err(bad(&naming::machine_refusal(name)));
        }
        Ok(())
    }

    fn plan_add(&self, args: &Value, mine: &str) -> Result<Plan, ApiError> {
        let machine = self.added_machine(args, mine)?;
        let name = machine.name.clone();
        Ok(Plan {
            action: "add",
            target: format!("add:{name}"),
            change: json!({"action": "add", "args": args, "apply": apply_of()}),
            facts: json!([
                self.machine_fact(&machine),
                {"line": "consent-machine-add", "args": {"machine": name}},
            ]),
        })
    }

    pub fn imported(&self, args: &Value, mine: &str) -> Result<Vec<Machine>, ApiError> {
        known_keys(args, &["peer", "user"])?;
        let peer = text(args, "peer").ok_or_else(|| bad(USAGE))?;
        let user = text(args, "user").ok_or_else(|| bad(USAGE))?;
        let status = self.tailscale.status().map_err(ApiError::internal)?;
        let made = import(
            &status,
            &[Picked {
                host: peer.to_string(),
                user: user.to_string(),
                key: None,
            }],
            &self.all(),
            organization_value(mine).as_deref(),
        )
        .map_err(|why| bad(&why).with_reason("no_peer"))?;
        if made.is_empty() {
            return Err(bad("That machine is already added.").with_reason("already_added"));
        }
        Ok(made)
    }

    fn plan_import(&self, args: &Value, mine: &str) -> Result<Plan, ApiError> {
        let made = self.imported(args, mine)?;
        let names: Vec<&str> = made.iter().map(|m| m.name.as_str()).collect();
        let mut facts: Vec<Value> = made.iter().map(|m| self.machine_fact(m)).collect();
        facts.push(
            json!({"line": "consent-machine-import", "args": {"machines": names.join(", ")}}),
        );
        Ok(Plan {
            action: "import",
            target: format!("import:{}", names.join(",")),
            change: json!({
                "action": "import",
                "args": args,
                "machines": names,
                "apply": apply_of(),
            }),
            facts: json!(facts),
        })
    }

    fn typed_address(typed: &str) -> Result<MachineAddress, ApiError> {
        let (user, host) = match typed.split_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, typed),
        };
        let mut a = MachineAddress::new(infer_kind(host), host);
        if let Some(u) = user.filter(|u| !u.is_empty()) {
            a = a.with_user(u);
        }
        if a.kind == AddressKind::Jump {
            return Err(bad("A jump address is set in the app.").with_reason("bad_value"));
        }
        Ok(a.normalized())
    }

    pub fn typed_list(
        &self,
        args: &Value,
        mine: &str,
    ) -> Result<(Machine, Vec<MachineAddress>), ApiError> {
        known_keys(args, &["machine", "addresses"])?;
        let name = text(args, "machine").ok_or_else(|| bad(USAGE))?;
        let m = self.owned(name, mine, never)?;
        let typed: Vec<&str> = args
            .get("addresses")
            .and_then(Value::as_array)
            .ok_or_else(|| bad(USAGE))?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        if typed.is_empty() || typed.len() > 8 {
            return Err(bad(USAGE));
        }
        let list: Vec<MachineAddress> = typed
            .iter()
            .map(|t| Self::typed_address(t))
            .collect::<Result<_, _>>()?;
        if list
            .iter()
            .any(|a| a.user.as_deref().is_some_and(|u| u != m.user))
        {
            return Err(forbidden(
                "A machine logs in as one user. Use the app to change it.",
                "login_user",
            )
            .with_field("never", json!(true)));
        }
        if let Some(why) = list_problems(&list, &m.name).into_iter().next() {
            return Err(bad(&why).with_reason("bad_value"));
        }
        Ok((m, list))
    }

    fn plan_addresses(&self, args: &Value, mine: &str) -> Result<Plan, ApiError> {
        let (m, list) = self.typed_list(args, mine)?;
        let after = list
            .iter()
            .map(|a| format!("{}@{}", a.user.as_deref().unwrap_or(&m.user), a.host))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Plan {
            action: "addresses",
            target: format!("addresses:{}", m.name),
            change: json!({"action": "addresses", "args": args, "apply": apply_of()}),
            facts: json!([
                self.machine_fact(&m),
                {"line": "consent-machine-addresses", "args": {"before": address_line(&m), "after": after}},
            ]),
        })
    }

    pub fn is_member(&self, m: &Machine) -> bool {
        let Some(node) = self.members.node_of(&m.name) else {
            return false;
        };
        self.members.status()["members"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|x| x["id"].as_str() == Some(node.as_str()) && x["state"] != json!("removed"))
    }

    fn plan_remove(&self, args: &Value, mine: &str) -> Result<Plan, ApiError> {
        known_keys(args, &["machine"])?;
        let name = text(args, "machine").ok_or_else(|| bad(USAGE))?;
        let m = self.owned(name, mine, never)?;
        if self.is_member(&m) {
            return Err(forbidden(
                "That machine is in the cluster. Use notchling cluster remove.",
                "cluster_member",
            )
            .with_field("pointsAt", json!("cluster")));
        }
        Ok(Plan {
            action: "remove",
            target: format!("remove:{}", m.name),
            change: json!({"action": "remove", "args": args, "apply": apply_of()}),
            facts: json!([
                self.machine_fact(&m),
                {"line": "consent-machine-remove", "args": {"machine": m.name}},
            ]),
        })
    }

    pub fn run_kind(&self, action: &str, args: &Value, m: &Machine) -> Result<PlanKind, ApiError> {
        match action {
            "fix" => {
                known_keys(args, &["machine", "step"])?;
                let step = text(args, "step").ok_or_else(|| bad(USAGE))?;
                let id = self
                    .engine
                    .steps(m)
                    .into_iter()
                    .map(|s| s.id)
                    .find(|id| id.raw() == step);
                match id {
                    Some(id) => Ok(PlanKind::Fix(id)),
                    None => match [
                        StepId::ISSUES_TOKEN,
                        StepId::TOKEN_SCOPE,
                        StepId::CLAUDE_LOGIN,
                        StepId::NODE_CODEX_LOGIN,
                        StepId::NODE_AGY_LOGIN,
                        StepId::NODE_JOINED,
                    ]
                    .into_iter()
                    .find(|s| s.raw() == step)
                    {
                        Some(id) => Ok(PlanKind::Fix(id)),
                        None => Err(bad("That machine has no such step.").with_reason("no_step")),
                    },
                }
            }
            "setup" => {
                known_keys(args, &["machine"])?;
                Ok(PlanKind::SetUpAll)
            }
            _ => {
                known_keys(args, &["machine", "on"])?;
                let on = args
                    .get("on")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| bad(USAGE))?;
                Ok(PlanKind::Running(on))
            }
        }
    }

    pub fn shown_plan(&self, m: &Machine, kind: &PlanKind) -> Result<ShownPlan, ApiError> {
        if let Some(family) = kind_refusal(kind) {
            return Err(never_at(family));
        }
        let shown = self
            .engine
            .plan(&m.name, kind.clone())
            .map_err(|why| bad(&why).with_reason("no_plan"))?;
        if let Some(family) = refusal(&shown) {
            self.engine.cancel(shown.id);
            return Err(never_at(family));
        }
        if let AdminNeed::Refused(why) = self.engine.admin_need_of(&shown) {
            self.engine.cancel(shown.id);
            return Err(bad(&why).with_reason("refused"));
        }
        Ok(shown)
    }

    fn plan_run(&self, action: &'static str, args: &Value, mine: &str) -> Result<Plan, ApiError> {
        let name = text(args, "machine").ok_or_else(|| bad(USAGE))?;
        let m = self.owned(name, mine, never)?;
        let kind = self.run_kind(action, args, &m)?;
        let shown = self.shown_plan(&m, &kind)?;
        self.engine.cancel(shown.id);
        let steps: Vec<Value> = shown
            .steps
            .iter()
            .map(|s| json!({"id": s.id.raw(), "title": s.title, "display": s.display}))
            .collect();
        let mut facts = vec![self.machine_fact(&m)];
        for s in &shown.steps {
            facts.push(json!({"line": "consent-machine-step", "args": {
                "title": s.title, "display": s.display}}));
        }
        let need = need_word(&self.engine.admin_need_of(&shown));
        facts.push(json!({"line": "consent-machine-admin", "args": {"need": need}}));
        let mut change = Map::new();
        change.insert("action".into(), json!(action));
        change.insert("args".into(), args.clone());
        change.insert("machine".into(), json!(m.name));
        change.insert("digest".into(), json!(digest(&shown)));
        change.insert("steps".into(), json!(steps));
        change.insert("admin".into(), json!(need));
        if need == "password" {
            if let Some(entry) = m.admin_secret() {
                change.insert("adminEntry".into(), json!(entry));
            }
        }
        change.insert("apply".into(), apply_of());
        Ok(Plan {
            action,
            target: format!("{action}:{}", m.name),
            change: Value::Object(change),
            facts: json!(facts),
        })
    }

    pub fn network_target(&self, wanted: &str) -> Result<(Network, Network), ApiError> {
        let list = self.tailscale.networks().map_err(ApiError::internal)?;
        let found: Vec<&Network> = list
            .iter()
            .filter(|n| n.is(wanted) || n.id == wanted)
            .collect();
        let to = match found.as_slice() {
            [one] => (*one).clone(),
            [] => {
                return Err(bad("That isn't one of this Mac's Tailscale networks.")
                    .with_reason("no_network"))
            }
            _ => {
                return Err(bad("Two networks have that name; use the id.").with_reason("ambiguous"))
            }
        };
        let from = list
            .iter()
            .find(|n| n.active)
            .cloned()
            .ok_or_else(|| bad("No Tailscale network is active.").with_reason("no_network"))?;
        if to.active {
            return Err(bad("That network is already active.").with_reason("already_set"));
        }
        Ok((from, to))
    }

    fn plan_network(&self, args: &Value) -> Result<Plan, ApiError> {
        known_keys(args, &["tailnet"])?;
        let wanted = text(args, "tailnet").ok_or_else(|| bad(USAGE))?;
        let (from, to) = self.network_target(wanted)?;
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for m in self.all() {
            let list = m.address_list();
            let only_tailscale = !list.is_empty()
                && list.iter().all(|a| {
                    a.kind.is_tailscale()
                        && a.tailnet_name()
                            .is_none_or(|t| t == from.tailnet || t.is_empty())
                });
            if only_tailscale {
                groups
                    .entry(m.organization_id().to_string())
                    .or_default()
                    .push(m.name.clone());
            }
        }
        let total: usize = groups.values().map(Vec::len).sum();
        let counts: Vec<Value> = groups
            .iter()
            .map(|(org, names)| json!({"organization": org, "count": names.len()}))
            .collect();
        let drops: Vec<Value> = groups
            .iter()
            .map(|(org, names)| json!({"organization": org, "machines": names}))
            .collect();
        let members = self.members.status()["members"]
            .as_array()
            .map(|a| a.iter().filter(|m| m["state"] != json!("removed")).count())
            .unwrap_or(0);
        let failover = self.members.status()["failover"]["value"]
            .as_str()
            .unwrap_or("off")
            .to_string();
        Ok(Plan {
            action: "network",
            target: format!("network:{}", to.id),
            change: json!({
                "action": "network",
                "args": args,
                "from": from.id, "to": to.id, "lose": total, "counts": counts,
                "apply": {"op": ops::TAILSCALE_SWITCH, "args": {"tailnet": to.id}},
            }),
            facts: json!([
                {"line": "consent-network-switch", "args": {
                    "from": mask_email(&from.tailnet), "to": mask_email(&to.tailnet)}},
                {"line": "consent-network-drops", "args": {"count": total, "groups": drops}},
                {"line": "consent-network-cluster", "args": {"members": members, "failover": failover}},
                {"line": "consent-network-phone", "args": {}},
            ]),
        })
    }
}
