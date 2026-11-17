use crate::acting_ops::bad;
use crate::cluster_node::InviteJoin;
use crate::inbox_ops::InboxOps;
use notchling_engine::cluster::invite::Invite;
use notchling_engine::cluster::text::{self, NAME_MAX};
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{Item, Kind, Source, State};
use notchling_engine::machines::forms::name_problem;
use notchling_local_api::ApiError;
use serde_json::Value;

const USAGE: &str = "cluster.invite takes {\"node\": id, \"name\": text, \"canBeMaster\": bool?, \"acting\": bool?}";

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn flag(args: &Value, key: &str) -> Result<bool, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| bad(USAGE)),
    }
}

pub fn parse_invite(args: &Value) -> Result<Invite, ApiError> {
    let node = args
        .get("node")
        .and_then(Value::as_str)
        .and_then(NodeId::parse)
        .ok_or_else(|| bad("The node must be a machine's 16 character id"))?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| bad(USAGE))?;
    if let Some(problem) = name_problem(name) {
        return Err(bad(problem));
    }
    Ok(Invite {
        node,
        name: name.to_string(),
        can_be_master: flag(args, "canBeMaster")?,
        acting: flag(args, "acting")?,
    })
}

fn plain(value: &str) -> String {
    text::plain(value, NAME_MAX)
}

fn os_title(os: &str) -> String {
    match plain(os).as_str() {
        "macos" => "macOS".to_string(),
        "linux" => "Linux".to_string(),
        other => other.to_string(),
    }
}

pub fn invite_item(joined: &InviteJoin, created_ms: i64) -> Item {
    let name = match plain(&joined.name) {
        name if name.is_empty() => joined.node.to_string(),
        name => name,
    };
    let maker = match plain(&joined.minter_name) {
        name if name.is_empty() => joined.minter.to_string(),
        name => name,
    };
    Item {
        id: format!("invite-joined:{}:{created_ms}", joined.node),
        kind: Kind::Notice,
        title: format!("{name} joined the cluster"),
        detail: format!(
            "Added over SSH from {maker} · {} {} · {}",
            os_title(&joined.os),
            plain(&joined.arch),
            joined.node
        ),
        source: Source {
            node: Some(joined.node.to_string()),
            session: None,
            job: None,
        },
        created_ms,
        expires_ms: None,
        actions: Vec::new(),
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
    }
}

pub fn raise_joined(inbox: &InboxOps, joined: &InviteJoin) {
    inbox.raise(invite_item(joined, now_ms()), None);
}

#[cfg(test)]
#[path = "cluster_invite_tests.rs"]
mod tests;
