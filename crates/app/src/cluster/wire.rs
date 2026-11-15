use notchling_engine::cluster::Desired;
use notchling_local_api::ops;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub op: &'static str,
    pub args: Value,
}

pub fn status() -> Request {
    Request {
        op: ops::CLUSTER_STATUS,
        args: json!({}),
    }
}

pub fn create(name: &str) -> Request {
    Request {
        op: ops::CLUSTER_CREATE,
        args: json!({"name": name.trim()}),
    }
}

pub fn join_token() -> Request {
    Request {
        op: ops::CLUSTER_JOIN_TOKEN,
        args: json!({}),
    }
}

pub fn can_be_master(node: &str, on: bool) -> Request {
    Request {
        op: ops::CLUSTER_MEMBER_SET,
        args: json!({"node": node, "canBeMaster": on}),
    }
}

pub fn desired(node: &str, desired: Desired) -> Request {
    Request {
        op: ops::CLUSTER_MEMBER_SET,
        args: json!({
            "node": node,
            "desired": {
                "acting": desired.acting,
                "bridge": desired.bridge,
                "install": desired.install,
            },
        }),
    }
}

pub fn allow_set(node: &str, allow: &Value) -> Request {
    let mut args = json!({"node": node});
    for key in ["names", "any"] {
        if let Some(value) = allow.get(key) {
            args[key] = value.clone();
        }
    }
    Request {
        op: ops::CLUSTER_ALLOW_SET,
        args,
    }
}

pub fn remove(node: &str) -> Request {
    Request {
        op: ops::CLUSTER_REMOVE,
        args: json!({"node": node}),
    }
}

pub fn turn_on() -> Request {
    Request {
        op: ops::SETTINGS_SET_SWITCH,
        args: json!({"switch": "cluster", "on": true}),
    }
}

pub fn token_of(reply: &Value) -> Option<String> {
    reply
        .get("token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}
