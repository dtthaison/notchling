use std::path::PathBuf;

use notchling_engine::machines::step::ClusterTokens;
use notchling_local_api::{ops, Client, ClientError, ClientKind, ConnectOptions, CALL_TIMEOUT};
use serde_json::{json, Value};

pub const NOT_RUNNING: &str = "Notchling isn't running on this Mac";

pub struct DaemonTokens {
    pub socket: PathBuf,
}

impl DaemonTokens {
    fn call(&self, op: &str) -> Result<Value, String> {
        let options = ConnectOptions {
            timeout: CALL_TIMEOUT,
            ..ConnectOptions::default()
        };
        let mut client = Client::connect_with(
            &self.socket,
            ClientKind::App,
            env!("CARGO_PKG_VERSION"),
            &options,
        )
        .map_err(|_| NOT_RUNNING.to_string())?;
        client.call(op, json!({})).map_err(|e| match e {
            ClientError::Api(api) => api.message,
            ClientError::Protocol(why) => why,
            _ => NOT_RUNNING.to_string(),
        })
    }

    fn status(&self) -> Option<Value> {
        self.call(ops::CLUSTER_STATUS).ok()
    }
}

impl ClusterTokens for DaemonTokens {
    fn join_token(&self) -> Result<String, String> {
        let reply = self.call(ops::CLUSTER_JOIN_TOKEN)?;
        token_in(&reply).ok_or_else(|| "the cluster gave no join token".to_string())
    }

    fn cluster_id(&self) -> Option<String> {
        cluster_id_in(&self.status()?)
    }

    fn can_be_master(&self, machine: &str) -> Option<bool> {
        can_be_master_in(&self.status()?, machine, None)
    }

    fn can_be_master_of(&self, machine: &str, node: Option<&str>) -> Option<bool> {
        can_be_master_in(&self.status()?, machine, node)
    }
}

pub fn token_in(reply: &Value) -> Option<String> {
    reply
        .get("token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

pub fn cluster_id_in(status: &Value) -> Option<String> {
    status
        .get("cluster")?
        .get("id")?
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

pub fn can_be_master_in(status: &Value, machine: &str, node: Option<&str>) -> Option<bool> {
    let members = status.get("members")?.as_array()?;
    let by_id = node.and_then(|id| {
        members
            .iter()
            .find(|m| m.get("id").and_then(Value::as_str) == Some(id))
    });
    let member = by_id.or_else(|| {
        members
            .iter()
            .find(|m| m.get("name").and_then(Value::as_str) == Some(machine))
    })?;
    member.get("canBeMaster").and_then(Value::as_bool)
}
