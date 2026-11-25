use super::*;
use crate::cluster_invite::parse_invite;
use notchling_engine::cluster::main_sessions::valid_id;
use notchling_engine::naming;

struct Owner {
    node: String,
    machine: String,
    host: String,
    organization: String,
}

impl Owner {
    fn json(&self) -> Value {
        json!({
            "node": self.node, "machine": self.machine,
            "host": self.host, "organization": self.organization,
        })
    }
}

struct Target {
    cluster: Value,
    member: Value,
    id: String,
    name: String,
    owner: Owner,
}

fn active(status: &Value) -> impl Iterator<Item = &Value> {
    status["members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["state"] != json!("removed"))
}

impl ClusterAsks {
    pub(super) fn member(&self, status: &Value, asked: &str) -> Result<Value, ApiError> {
        let found = active(status)
            .find(|m| m["id"].as_str() == Some(asked))
            .or_else(|| {
                active(status).find(|m| {
                    m["name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case(asked))
                })
            })
            .cloned();
        found.ok_or_else(|| bad("No such machine in the cluster.").with_reason("no_machine"))
    }

    fn same_org(&self, mine: &str, theirs: &str) -> Result<(), ApiError> {
        if mine == theirs {
            Ok(())
        } else {
            Err(forbidden(OTHER_ORG, "other_organization"))
        }
    }

    fn in_cluster(status: &Value) -> Result<Value, ApiError> {
        let cluster = &status["cluster"];
        if cluster.is_null() {
            return Err(bad(NO_CLUSTER).with_reason("no_cluster"));
        }
        Ok(json!({"id": cluster["id"], "name": cluster["name"]}))
    }

    fn owner(
        &self,
        status: &Value,
        member: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Owner, ApiError> {
        let node = member["id"].as_str().unwrap_or_default().to_string();
        if !node.is_empty() && status["me"].as_str() == Some(node.as_str()) {
            let organization = self.admin.own_organization().ok_or_else(unknown_owner)?;
            self.same_org(mine, &organization)?;
            return Ok(Owner {
                node,
                machine: String::new(),
                host: String::new(),
                organization,
            });
        }
        let name = member["name"].as_str().unwrap_or_default();
        if node.is_empty() || self.admin.matches(name) != 1 {
            return Err(unknown_owner());
        }
        let organization = self.admin.organization_of(name).ok_or_else(unknown_owner)?;
        self.same_org(mine, &organization)?;
        if look == Look::Probe && self.admin.node_of(name).as_deref() != Some(node.as_str()) {
            return Err(unknown_owner());
        }
        Ok(Owner {
            node,
            machine: name.to_string(),
            host: self.admin.host_of(name).unwrap_or_default(),
            organization,
        })
    }

    fn primary(&self, status: &Value, mine: &str, look: Look) -> Result<Owner, ApiError> {
        let home = status["cluster"]["home"]
            .as_str()
            .ok_or_else(unknown_owner)?;
        let member = active(status)
            .find(|m| m["id"].as_str() == Some(home))
            .ok_or_else(unknown_owner)?;
        self.owner(status, member, mine, look)
    }

    fn wide(&self, status: &Value, mine: &str, look: Look) -> Result<(Value, Owner), ApiError> {
        let cluster = Self::in_cluster(status)?;
        let primary = self.primary(status, mine, look)?;
        Ok((cluster, primary))
    }

    fn target(
        &self,
        status: &Value,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Target, ApiError> {
        let cluster = Self::in_cluster(status)?;
        let asked = text(args, "node").ok_or_else(|| bad(USAGE))?;
        let member = self.member(status, asked)?;
        let name = member["name"].as_str().unwrap_or_default().to_string();
        let id = member["id"].as_str().unwrap_or_default().to_string();
        let owner = self.owner(status, &member, mine, look)?;
        Ok(Target {
            cluster,
            member,
            id,
            name,
            owner,
        })
    }

    fn machine_facts(&self, member: &Value, owner: &Owner) -> Value {
        let name = member["name"].as_str().unwrap_or_default();
        let address = Some(owner.host.clone())
            .filter(|host| !host.is_empty())
            .or_else(|| {
                let ip = member["addrs"]
                    .as_array()
                    .and_then(|a| a.first())?
                    .as_str()?;
                Some(match member["port"].as_u64() {
                    Some(port) => format!("{ip}:{port}"),
                    None => ip.to_string(),
                })
            })
            .unwrap_or_default();
        json!({"line": "consent-cluster-machine", "args": {
            "machine": name, "address": address, "organization": owner.organization,
        }})
    }

    fn cluster_fact(cluster: &Value) -> Value {
        json!({"line": "consent-cluster", "args": {"cluster": cluster["name"]}})
    }

    pub(super) fn name_refusal(&self, action: &str, args: &Value) -> Result<(), ApiError> {
        let Some(name) = text(args, "name") else {
            return Ok(());
        };
        match action {
            "create" if naming::cluster_generic(name) => Err(bad(&naming::cluster_refusal(name))),
            "invite" if naming::machine_generic(name) && self.admin.matches(name) == 0 => {
                Err(bad(&naming::machine_refusal(name)))
            }
            _ => Ok(()),
        }
    }

    pub(super) fn plan_create(&self, args: &Value, mine: &str) -> Result<Plan, ApiError> {
        known_keys(args, &["name"])?;
        let name = text(args, "name").ok_or_else(|| bad(USAGE))?;
        if name.chars().count() > NAME_CHARS || name.chars().any(char::is_control) {
            return Err(bad("The cluster's name is too long or has odd characters.")
                .with_reason("bad_value"));
        }
        let status = self.admin.status();
        if !status["cluster"].is_null() || !status["joining"].is_null() {
            return Err(bad("This machine is already in a cluster.").with_reason("already_in"));
        }
        Ok(Plan {
            action: "create",
            target: "create".to_string(),
            change: json!({
                "action": "create", "name": name, "organization": mine,
                "args": args,
                "apply": {"op": ops::CLUSTER_CREATE, "args": {"name": name}},
            }),
            facts: json!([{"line": "consent-cluster-create", "args": {
                "name": name, "organization": mine,
            }}]),
        })
    }

    pub(super) fn plan_token(
        &self,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        known_keys(args, &[])?;
        let status = self.admin.status();
        let (cluster, primary) = self.wide(&status, mine, look)?;
        Ok(Plan {
            action: "token",
            target: "token".to_string(),
            change: json!({
                "action": "token", "cluster": cluster["name"], "clusterId": cluster["id"],
                "organization": primary.organization, "primary": primary.json(), "args": args,
                "apply": {"op": ops::CLUSTER_JOIN_TOKEN, "args": {}},
            }),
            facts: json!([
                Self::cluster_fact(&cluster),
                {"line": "consent-cluster-token", "args": {}},
            ]),
        })
    }

    pub(super) fn plan_invite(
        &self,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        known_keys(args, &["node", "name", "canBeMaster", "acting"])?;
        let invite = parse_invite(args)?;
        let status = self.admin.status();
        let cluster = Self::in_cluster(&status)?;
        let node = invite.node.as_str().to_string();
        if active(&status).any(|m| m["id"].as_str() == Some(&node)) {
            return Err(bad("That machine is already in the cluster.").with_reason("already_in"));
        }
        let name_taken = active(&status).any(|m| {
            m["id"].as_str() == Some(invite.name.as_str())
                || m["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&invite.name))
        });
        if name_taken {
            return Err(
                bad("A machine in the cluster already has that name.").with_reason("already_in")
            );
        }
        match self.admin.matches(&invite.name) {
            0 => return Err(bad("That name is no machine in Machines.").with_reason("no_machine")),
            1 => {}
            _ => return Err(unknown_owner()),
        }
        let organization = self
            .admin
            .organization_of(&invite.name)
            .ok_or_else(unknown_owner)?;
        self.same_org(mine, &organization)?;
        if look == Look::Probe && self.admin.node_of(&invite.name).as_deref() != Some(node.as_str())
        {
            return Err(bad("That machine is not the node named.").with_reason("wrong_node"));
        }
        let address = self.admin.host_of(&invite.name).unwrap_or_default();
        let apply = json!({
            "node": node, "name": invite.name,
            "canBeMaster": invite.can_be_master, "acting": invite.acting,
        });
        Ok(Plan {
            action: "invite",
            target: format!("invite:{node}"),
            change: json!({
                "action": "invite", "clusterId": cluster["id"],
                "machine": invite.name, "node": node,
                "organization": organization, "canBeMaster": invite.can_be_master,
                "acting": invite.acting, "args": apply,
                "apply": {"op": ops::CLUSTER_INVITE, "args": apply},
            }),
            facts: json!([
                Self::cluster_fact(&cluster),
                {"line": "consent-cluster-invite", "args": {
                    "name": invite.name, "node": node,
                    "role": role_word(invite.can_be_master),
                    "acting": on_word(invite.acting),
                }},
                {"line": "consent-cluster-machine", "args": {
                    "machine": invite.name, "address": address, "organization": organization,
                }},
            ]),
        })
    }

    pub(super) fn plan_member(
        &self,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        known_keys(args, &["node", "role", "delegation"])?;
        let status = self.admin.status();
        let t = self.target(&status, args, mine, look)?;
        let role = match text(args, "role") {
            None => None,
            Some("master") => Some(true),
            Some("worker") => Some(false),
            Some(_) => return Err(bad(USAGE)),
        };
        let delegation = flag(args, "delegation")?;
        if role.is_none() && delegation.is_none() {
            return Err(bad(USAGE));
        }
        let was_master = t.member["canBeMaster"].as_bool() == Some(true);
        let was_delegating = t.member["acceptsDelegation"].as_bool() == Some(true);
        let mut apply = Map::new();
        apply.insert("node".into(), json!(t.id));
        let mut facts = vec![
            Self::cluster_fact(&t.cluster),
            self.machine_facts(&t.member, &t.owner),
        ];
        let mut change = json!({
            "action": "member", "clusterId": t.cluster["id"], "machine": t.name, "node": t.id,
            "organization": t.owner.organization, "args": args,
        });
        let mut moved = false;
        if let Some(master) = role.filter(|m| *m != was_master) {
            apply.insert("canBeMaster".into(), json!(master));
            change["role"] = json!({"before": role_word(was_master), "after": role_word(master)});
            facts.push(json!({"line": "consent-cluster-role", "args": {
                "before": role_word(was_master), "after": role_word(master),
            }}));
            moved = true;
        }
        if let Some(on) = delegation.filter(|d| *d != was_delegating) {
            apply.insert("acceptsDelegation".into(), json!(on));
            change["delegation"] = json!({"before": on_word(was_delegating), "after": on_word(on)});
            facts.push(json!({"line": "consent-cluster-delegation", "args": {
                "before": on_word(was_delegating), "after": on_word(on),
            }}));
            moved = true;
        }
        if !moved {
            return Err(bad("Nothing would change.").with_reason("already_set"));
        }
        change["apply"] = json!({"op": ops::CLUSTER_MEMBER_SET, "args": Value::Object(apply)});
        Ok(Plan {
            action: "member",
            target: format!("member:{}", t.id),
            change,
            facts: Value::Array(facts),
        })
    }

    pub(super) fn plan_failover(
        &self,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        known_keys(args, &["value"])?;
        let after = text(args, "value")
            .and_then(failover_value)
            .ok_or_else(|| bad(USAGE))?;
        let status = self.admin.status();
        let (cluster, primary) = self.wide(&status, mine, look)?;
        let before = status["failover"]["value"].as_str().unwrap_or("off");
        if before == after {
            return Err(bad("It is already set that way.").with_reason("already_set"));
        }
        Ok(Plan {
            action: "failover",
            target: "failover".to_string(),
            change: json!({
                "action": "failover", "clusterId": cluster["id"],
                "organization": primary.organization, "primary": primary.json(),
                "before": before, "after": after, "args": args,
                "apply": {"op": ops::CLUSTER_FAILOVER_SET, "args": {"value": after}},
            }),
            facts: json!([
                Self::cluster_fact(&cluster),
                {"line": "consent-cluster-failover", "args": {"before": before, "after": after}},
            ]),
        })
    }

    pub(super) fn plan_main(&self, args: &Value, mine: &str, look: Look) -> Result<Plan, ApiError> {
        known_keys(args, &["session", "on"])?;
        let session = text(args, "session")
            .filter(|s| valid_id(s))
            .ok_or_else(|| bad(USAGE))?;
        let on = flag(args, "on")?.unwrap_or(true);
        let status = self.admin.status();
        Self::in_cluster(&status)?;
        if !crate::cluster_main::is_home(&status) {
            return Err(
                bad("Only the primary knows which sessions are main.").with_reason("on_home_only")
            );
        }
        let (cluster, primary) = self.wide(&status, mine, look)?;
        let was = status["main"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|s| s.as_str() == Some(session));
        if was == on {
            return Err(bad("It is already set that way.").with_reason("already_set"));
        }
        Ok(Plan {
            action: "main",
            target: format!("main:{session}"),
            change: json!({
                "action": "main", "clusterId": cluster["id"],
                "organization": primary.organization, "primary": primary.json(),
                "session": session, "before": on_word(was), "after": on_word(on),
                "args": {"session": session, "on": on},
                "apply": {"op": ops::CLUSTER_MAIN_SET, "args": {"id": session, "main": on}},
            }),
            facts: json!([
                Self::cluster_fact(&cluster),
                {"line": "consent-cluster-main", "args": {
                    "session": session, "before": on_word(was), "after": on_word(on),
                }},
            ]),
        })
    }

    pub(super) fn plan_allow(
        &self,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        known_keys(args, &["node", "names"])?;
        let status = self.admin.status();
        let t = self.target(&status, args, mine, look)?;
        let listed = args
            .get("names")
            .and_then(Value::as_array)
            .ok_or_else(|| bad(USAGE))?;
        if listed.len() > ALLOW_MAX {
            return Err(bad(USAGE));
        }
        let mut names: Vec<String> = Vec::new();
        for name in listed {
            let name = name
                .as_str()
                .ok_or_else(|| bad(USAGE))?
                .trim()
                .to_lowercase();
            if name.is_empty() {
                return Err(bad(USAGE));
            }
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.sort();
        let theirs = t.owner.organization.as_str();
        for name in &names {
            let org = self
                .admin
                .secret_organization(name)
                .ok_or_else(|| bad("No such secret.").with_reason("no_secret"))?;
            self.same_org(mine, &org)?;
            self.same_org(theirs, &org)?;
        }
        let allowed = &t.member["secrets"];
        let mut before: Vec<String> = allowed["names"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_str().map(str::to_string))
            .collect();
        before.sort();
        let any = allowed["any"].as_bool() == Some(true);
        if before == names && !any {
            return Err(bad("It is already set that way.").with_reason("already_set"));
        }
        let mut facts = vec![
            Self::cluster_fact(&t.cluster),
            self.machine_facts(&t.member, &t.owner),
        ];
        if names.is_empty() {
            facts.push(json!({"line": "consent-cluster-allow-none", "args": {}}));
        }
        for name in &names {
            facts.push(json!({"line": "consent-cluster-allow-secret", "args": {"name": name}}));
        }
        let id = &t.id;
        Ok(Plan {
            action: "allow",
            target: format!("allow:{id}"),
            change: json!({
                "action": "allow", "clusterId": t.cluster["id"], "machine": t.name, "node": id,
                "organization": t.owner.organization, "before": before, "after": names,
                "beforeAny": any,
                "args": {"node": id, "names": names},
                "apply": {"op": ops::CLUSTER_ALLOW_SET, "args": {"node": id, "names": names, "any": false}},
            }),
            facts: Value::Array(facts),
        })
    }

    pub(super) fn plan_remove(
        &self,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        known_keys(args, &["node"])?;
        let status = self.admin.status();
        let t = self.target(&status, args, mine, look)?;
        let role = t.member["role"].as_str().unwrap_or("worker");
        let id = &t.id;
        Ok(Plan {
            action: "remove",
            target: format!("remove:{id}"),
            change: json!({
                "action": "remove", "clusterId": t.cluster["id"], "machine": t.name, "node": id,
                "organization": t.owner.organization, "role": role,
                "args": {"node": id},
                "apply": {"op": ops::CLUSTER_REMOVE, "args": {"node": id}},
            }),
            facts: json!([
                Self::cluster_fact(&t.cluster),
                self.machine_facts(&t.member, &t.owner),
                {"line": "consent-cluster-remove", "args": {"machine": t.name}},
            ]),
        })
    }

    pub(super) fn plan_move_home(
        &self,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        known_keys(args, &["node"])?;
        let status = self.admin.status();
        let t = self.target(&status, args, mine, look)?;
        if status["cluster"]["home"].as_str() == Some(&t.id) {
            return Err(bad("That machine is already the primary.").with_reason("already_set"));
        }
        if t.member["canBeMaster"].as_bool() != Some(true) {
            return Err(bad("That machine can't be the primary.").with_reason("bad_value"));
        }
        let from = self.primary(&status, mine, look)?;
        let id = &t.id;
        Ok(Plan {
            action: "move_home",
            target: format!("move_home:{id}"),
            change: json!({
                "action": "move_home", "clusterId": t.cluster["id"],
                "machine": t.name, "node": id,
                "organization": t.owner.organization, "homeOrganization": from.organization,
                "primary": from.json(),
                "home": status["cluster"]["home"],
                "args": {"node": id},
                "apply": {"op": ops::CLUSTER_MOVE_HOME, "args": {"to": id}},
            }),
            facts: json!([
                Self::cluster_fact(&t.cluster),
                self.machine_facts(&t.member, &t.owner),
                {"line": "consent-cluster-move-home", "args": {"machine": t.name}},
            ]),
        })
    }

    pub(super) fn plan_move_now(
        &self,
        args: &Value,
        mine: &str,
        look: Look,
    ) -> Result<Plan, ApiError> {
        known_keys(args, &[])?;
        let status = self.admin.status();
        let (cluster, primary) = self.wide(&status, mine, look)?;
        Ok(Plan {
            action: "move_now",
            target: "move_now".to_string(),
            change: json!({
                "action": "move_now", "clusterId": cluster["id"],
                "organization": primary.organization, "primary": primary.json(),
                "args": args, "home": status["cluster"]["home"],
                "apply": {"op": ops::CLUSTER_MOVE_NOW, "args": {}},
            }),
            facts: json!([
                Self::cluster_fact(&cluster),
                {"line": "consent-cluster-move-now", "args": {}},
            ]),
        })
    }
}
