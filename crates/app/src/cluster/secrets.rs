use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use notchling_engine::cluster::grants::{SecretAllow, MAX_ALLOWED_NAMES};
use notchling_engine::vault::entry::valid_name;
use notchling_locale as t;
use serde_json::{json, Value};

use super::model::{ClusterModel, VaultNode};
use super::wire::Request;
use crate::switches::feed::SwitchFeed;
use crate::switches::{Switch, Switches};
use crate::vault::store::Vault;

pub const MAX_ATTACHED: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretRow {
    pub name: String,
    pub on: bool,
    pub in_vault: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretsModel {
    rows: Vec<SecretRow>,
    any: bool,
    start: (BTreeSet<String>, bool),
}

impl SecretsModel {
    pub fn new(names: &[String], allow: &SecretAllow) -> SecretsModel {
        let mut rows: BTreeMap<String, SecretRow> = BTreeMap::new();
        for name in names.iter().filter(|n| valid_name(n)) {
            rows.insert(
                name.to_lowercase(),
                SecretRow {
                    name: name.clone(),
                    on: allow.names.contains(&name.to_lowercase()),
                    in_vault: true,
                },
            );
        }
        for name in &allow.names {
            rows.entry(name.clone()).or_insert_with(|| SecretRow {
                name: name.clone(),
                on: true,
                in_vault: false,
            });
        }
        SecretsModel {
            rows: rows.into_values().collect(),
            any: allow.any,
            start: (allow.names.clone(), allow.any),
        }
    }

    pub fn rows(&self) -> &[SecretRow] {
        &self.rows
    }

    pub fn any(&self) -> bool {
        self.any
    }

    pub fn shows_list(&self) -> bool {
        !self.any
    }

    pub fn at_cap(&self) -> bool {
        self.chosen().len() >= MAX_ALLOWED_NAMES
    }

    pub fn toggle(&mut self, name: &str) {
        let at = self
            .rows
            .iter()
            .position(|r| r.name.eq_ignore_ascii_case(name));
        let on_now = self.chosen().len();
        if let Some(row) = at.map(|i| &mut self.rows[i]) {
            if !row.on && on_now >= MAX_ALLOWED_NAMES {
                return;
            }
            row.on = !row.on;
        }
        self.rows.retain(|r| r.on || r.in_vault);
    }

    pub fn set_any(&mut self, on: bool) {
        self.any = on;
    }

    fn chosen(&self) -> BTreeSet<String> {
        self.rows
            .iter()
            .filter(|r| r.on)
            .map(|r| r.name.to_lowercase())
            .collect()
    }

    pub fn is_changed(&self) -> bool {
        (self.chosen(), self.any) != self.start
    }

    pub fn request(&self) -> Value {
        json!({"names": self.chosen().into_iter().collect::<Vec<_>>(), "any": self.any})
    }
}

pub fn summary(allow: &SecretAllow) -> String {
    if allow.any {
        t::cluster::secrets_any_short().to_owned()
    } else if allow.names.is_empty() {
        t::cluster::secrets_none().to_owned()
    } else {
        t::cluster::secrets_count(allow.names.len())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretsPicker {
    offered: Vec<String>,
    chosen: BTreeSet<String>,
    wait: Option<String>,
}

impl SecretsPicker {
    pub fn new(
        vault_names: &[String],
        allow: &SecretAllow,
        vault: Option<&VaultNode>,
    ) -> SecretsPicker {
        let mut offered: Vec<String> = vault_names
            .iter()
            .filter(|n| valid_name(n) && allow.allows(n))
            .cloned()
            .collect();
        offered.sort_by_key(|n| n.to_lowercase());
        offered.dedup_by_key(|n| n.to_lowercase());
        SecretsPicker {
            offered,
            chosen: BTreeSet::new(),
            wait: vault
                .filter(|v| !v.primary)
                .map(|v| t::cluster::secrets_wait(&v.name)),
        }
    }

    pub fn offered(&self) -> &[String] {
        &self.offered
    }

    pub fn note(&self) -> Option<&str> {
        self.wait.as_deref()
    }

    pub fn is_empty(&self) -> bool {
        self.offered.is_empty()
    }

    pub fn at_cap(&self) -> bool {
        self.chosen.len() >= MAX_ATTACHED
    }

    pub fn is_on(&self, name: &str) -> bool {
        self.chosen.contains(&name.to_lowercase())
    }

    pub fn toggle(&mut self, name: &str) {
        let key = name.to_lowercase();
        if !self.offered.iter().any(|n| n.to_lowercase() == key) {
            return;
        }
        if !self.chosen.remove(&key) && self.chosen.len() < MAX_ATTACHED {
            self.chosen.insert(key);
        }
    }

    pub fn attach(&self) -> Vec<String> {
        self.chosen.iter().cloned().collect()
    }

    pub fn with_secrets(&self, mut args: Value) -> Value {
        if !self.chosen.is_empty() {
            args["secrets"] = json!(self.attach());
        }
        args
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Local,
    Remote(String),
    Unreachable,
}

pub fn target(remote_tab: bool, model: &ClusterModel) -> Target {
    if !remote_tab {
        return Target::Local;
    }
    match model.remote_vault() {
        Some(node) => Target::Remote(node.id.clone()),
        None => Target::Unreachable,
    }
}

pub fn store_for(
    pinned: Option<&Target>,
    remote_tab: bool,
    model: &ClusterModel,
    local: &Arc<Vault>,
    remote: &Arc<Vault>,
) -> Result<Arc<Vault>, String> {
    let now = target(remote_tab, model);
    if pinned.is_some_and(|p| *p != now) {
        return Err(t::cluster::vault_moved().to_owned());
    }
    match now {
        Target::Local => Ok(local.clone()),
        Target::Remote(id) => {
            remote.set_node(Some(&id));
            Ok(remote.clone())
        }
        Target::Unreachable => Err(t::cluster::vault_unreachable().to_owned()),
    }
}

pub fn save_request(
    model: &ClusterModel,
    node: &str,
    edit: &SecretsModel,
    live: bool,
) -> Result<Request, String> {
    if !live {
        return Err(crate::acting::DETACHED_MESSAGE.to_owned());
    }
    model
        .set_secrets(node, edit)
        .ok_or_else(|| t::cluster::secrets_node_gone().to_owned())
}

pub struct VaultAccess {
    pub local: Arc<Vault>,
    pub switches: Arc<SwitchFeed>,
}

pub fn vault_names(
    model: &ClusterModel,
    local: &Vault,
    remote: &Vault,
    switches: &Switches,
) -> Result<Vec<String>, String> {
    let Some(node) = model.vault_node() else {
        return Err(t::cluster::vault_unknown().to_owned());
    };
    let store = if node.me {
        if let Some(why) = switches.message(Switch::Vault) {
            return Err(why.to_owned());
        }
        local
    } else {
        if !node.online {
            return Err(t::cluster::vault_offline(&node.name));
        }
        remote.set_node(Some(&node.id));
        remote
    };
    store.refresh(switches);
    let snapshot = store.snapshot();
    match snapshot.error {
        Some(error) => Err(error),
        None if !snapshot.attached => Err(crate::acting::DETACHED_MESSAGE.to_owned()),
        None => Ok(snapshot.entries.into_iter().map(|e| e.name).collect()),
    }
}
