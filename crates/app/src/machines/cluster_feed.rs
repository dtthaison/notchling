use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notchling_engine::cluster::ready::{checks_in, Checks};
use notchling_engine::machines::step::ClusterTokens;
use notchling_local_api::{ops, Client, ClientError, ClientKind, ConnectOptions, CALL_TIMEOUT};
use serde_json::{json, Value};

pub const NOT_RUNNING: &str = "Notchling isn't running on this Mac";

const ID_TTL: Duration = Duration::from_secs(3);

type CachedId = (Instant, Result<Option<String>, String>);

pub struct DaemonTokens {
    socket: PathBuf,
    id: Mutex<Option<CachedId>>,
    seen: Mutex<Option<(Instant, Option<Value>)>>,
}

impl DaemonTokens {
    pub fn new(socket: PathBuf) -> DaemonTokens {
        DaemonTokens {
            socket,
            id: Mutex::new(None),
            seen: Mutex::new(None),
        }
    }

    fn recent_status(&self) -> Option<Value> {
        let mut cached = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, status)) = cached.as_ref() {
            if at.elapsed() < ID_TTL {
                return status.clone();
            }
        }
        let status = self.status().ok();
        *cached = Some((Instant::now(), status.clone()));
        status
    }

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

    fn status(&self) -> Result<Value, String> {
        self.call(ops::CLUSTER_STATUS)
    }
}

impl ClusterTokens for DaemonTokens {
    fn join_token(&self) -> Result<String, String> {
        let reply = self.call(ops::CLUSTER_JOIN_TOKEN)?;
        token_in(&reply).ok_or_else(|| "the cluster gave no join token".to_string())
    }

    fn cluster_id(&self) -> Result<Option<String>, String> {
        let mut cached = self.id.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, answer)) = cached.as_ref() {
            if at.elapsed() < ID_TTL {
                return answer.clone();
            }
        }
        let answer = self.status().map(|s| cluster_id_in(&s));
        *cached = Some((Instant::now(), answer.clone()));
        answer
    }

    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }

    fn master_checks(&self, node: Option<&str>) -> Option<Checks> {
        checks_in(&self.recent_status()?, node?)
    }

    fn can_be_master_of(&self, _machine: &str, node: Option<&str>) -> Option<bool> {
        can_be_master_in(&self.status().ok()?, node?)
    }

    fn can_be_master_all(
        &self,
        asks: &[(String, Option<String>)],
    ) -> BTreeMap<String, Option<bool>> {
        let status = self.status().ok();
        asks.iter()
            .map(|(name, node)| {
                let flag = status
                    .as_ref()
                    .zip(node.as_deref())
                    .and_then(|(s, id)| can_be_master_in(s, id));
                (name.clone(), flag)
            })
            .collect()
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

pub fn can_be_master_in(status: &Value, node: &str) -> Option<bool> {
    let members = status.get("members")?.as_array()?;
    let member = members
        .iter()
        .find(|m| m.get("id").and_then(Value::as_str) == Some(node))?;
    member.get("canBeMaster").and_then(Value::as_bool)
}
