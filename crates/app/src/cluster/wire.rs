use notchling_engine::cluster::Desired;
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::fmt;

pub const TOKEN_PREFIX: &str = "nlj1.";
const HIDDEN: &str = "[hidden]";

#[derive(Clone, PartialEq)]
pub struct Request {
    pub op: &'static str,
    pub args: Value,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut shown = self.args.clone();
        if let Some(token) = shown.get_mut("token") {
            *token = json!(HIDDEN);
        }
        f.debug_list().entry(&self.op).entry(&shown).finish()
    }
}

pub fn without_token(text: &str, args: &Value) -> String {
    match args.get("token").and_then(Value::as_str) {
        Some(token) if !token.is_empty() => text.replace(token, HIDDEN),
        _ => text.to_owned(),
    }
}

#[derive(Debug, PartialEq)]
pub enum JoinInput {
    Nothing,
    NotAToken,
    Send(Request),
}

impl JoinInput {
    pub fn clears_field(&self) -> bool {
        matches!(self, JoinInput::Send(_))
    }
}

pub fn join(typed: &str) -> JoinInput {
    let token = typed.trim();
    if token.is_empty() {
        JoinInput::Nothing
    } else if !token.starts_with(TOKEN_PREFIX) {
        JoinInput::NotAToken
    } else {
        JoinInput::Send(Request {
            op: ops::CLUSTER_JOIN,
            args: json!({"token": token}),
        })
    }
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
