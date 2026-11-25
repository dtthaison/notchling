use super::inbox_item::consent_of;
use crate::cluster::id::Qualified;
use crate::cluster::policy::cluster_may_set;
use crate::cluster::Desired;
use crate::inbox::{ActionId, Item, Kind};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerSwitch {
    pub node: String,
    pub switch: String,
    pub consent: String,
}

pub fn worker_switch(item: &Item) -> Option<WorkerSwitch> {
    if item.kind != Kind::Consent || item.open.is_some() {
        return None;
    }
    let switch = item
        .switch
        .as_deref()
        .filter(|name| cluster_may_set(name))?;
    let qualified = Qualified::parse(&item.id)?;
    let consent = consent_of(&qualified.id)?;
    Some(WorkerSwitch {
        node: qualified.node.to_string(),
        switch: switch.to_string(),
        consent: consent.to_string(),
    })
}

fn desired_of(status: &Value, node: &str) -> Option<Desired> {
    let member = status
        .get("members")
        .and_then(Value::as_array)?
        .iter()
        .find(|m| m.get("id").and_then(Value::as_str) == Some(node))?;
    Some(
        member
            .get("desired")
            .and_then(|d| serde_json::from_value(d.clone()).ok())
            .unwrap_or_default(),
    )
}

pub fn member_set_args(item: &Item, action: ActionId, status: &Value) -> Option<Value> {
    if action != ActionId::Allow {
        return None;
    }
    let asked = worker_switch(item)?;
    let mut desired = desired_of(status, &asked.node)?;
    match asked.switch.as_str() {
        "acting" => desired.acting = true,
        "bridge" => desired.bridge = true,
        "install" => desired.install = true,
        _ => return None,
    }
    Some(json!({
        "node": asked.node,
        "desired": desired,
        "consentId": asked.consent,
    }))
}

#[cfg(test)]
#[path = "worker_switch_tests.rs"]
mod tests;
