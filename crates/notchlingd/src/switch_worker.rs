use crate::ask_wiring::{worker_name, Slot};
use notchling_engine::cluster::policy::cluster_may_set;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Mac,
    Worker { name: String, linked: bool },
}

pub type Where = Arc<dyn Fn() -> Place + Send + Sync>;

fn home_online(status: &Value) -> bool {
    let Some(home) = status.pointer("/cluster/home").and_then(Value::as_str) else {
        return false;
    };
    status
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|m| {
            m.get("id").and_then(Value::as_str) == Some(home)
                && m.get("online").and_then(Value::as_bool) == Some(true)
        })
}

pub fn place_in(slot: Slot) -> Where {
    Arc::new(move || {
        let Some(node) = slot.get() else {
            return Place::Mac;
        };
        let Some(name) = node
            .state()
            .and_then(|state| worker_name(&state, node.me()))
        else {
            return Place::Mac;
        };
        Place::Worker {
            name,
            linked: home_online(&node.status()),
        }
    })
}

pub fn no_app(name: &str) -> ApiError {
    ApiError::new(
        ErrorCode::Forbidden,
        format!("Open Notchling on {name} to allow this."),
    )
    .with_reason("no_app")
}

pub fn refusal(place: &Place, app: bool, switch: &str) -> Option<ApiError> {
    match place {
        Place::Worker { name, linked } if !app && !(*linked && cluster_may_set(switch)) => {
            Some(no_app(name))
        }
        _ => None,
    }
}

pub fn headless(place: &Place, app: bool) -> bool {
    matches!(place, Place::Worker { .. }) && !app
}

pub fn forwarded(place: &Place, app: bool, switch: &str) -> bool {
    headless(place, app) && cluster_may_set(switch)
}

#[cfg(test)]
#[path = "switch_worker_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "switch_worker_cluster_tests.rs"]
mod cluster_tests;
