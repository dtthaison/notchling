use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notchling_engine::cluster::ready::{checks_in, Checks};
use notchling_engine::machines::process::ProcessRunner;
use notchling_engine::machines::runner::{Call, Runner};
use notchling_engine::machines::step::ClusterTokens;
use notchling_engine::machines::steps_node_reach::is_tailscale_ip;
use notchling_local_api::{
    ops, Client, ClientError, ClientKind, ConnectOptions, ErrorCode, CALL_TIMEOUT,
};
use serde_json::{json, Value};

pub const NOT_RUNNING: &str = "Notchling isn't running on this Mac";

const ID_TTL: Duration = Duration::from_secs(3);
const TAILNET_TTL: Duration = Duration::from_secs(60);
const TAILNET_NAME_MAX: usize = 253;
const TAILSCALE_CLI: [&str; 3] = [
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    "/opt/homebrew/bin/tailscale",
    "/usr/local/bin/tailscale",
];

pub type TailscaleRunner = Arc<dyn Fn() -> Option<String> + Send + Sync>;

type CachedId = (Instant, Result<Option<String>, String>);

pub struct DaemonTokens {
    socket: PathBuf,
    id: Mutex<Option<CachedId>>,
    seen: Mutex<Option<(Instant, Option<Value>)>>,
    tailscale: TailscaleRunner,
    tailnet: Mutex<Option<(Instant, Option<String>)>>,
}

impl DaemonTokens {
    pub fn new(socket: PathBuf) -> DaemonTokens {
        DaemonTokens::with_tailscale(socket, Arc::new(run_tailscale))
    }

    pub fn with_tailscale(socket: PathBuf, tailscale: TailscaleRunner) -> DaemonTokens {
        DaemonTokens {
            socket,
            id: Mutex::new(None),
            seen: Mutex::new(None),
            tailscale,
            tailnet: Mutex::new(None),
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
        self.call_with(op, json!({}))
    }

    fn connect(&self) -> Result<Client, String> {
        let options = ConnectOptions {
            timeout: CALL_TIMEOUT,
            ..ConnectOptions::default()
        };
        Client::connect_with(
            &self.socket,
            ClientKind::App,
            env!("CARGO_PKG_VERSION"),
            &options,
        )
        .map_err(|_| NOT_RUNNING.to_string())
    }

    fn call_with(&self, op: &str, args: Value) -> Result<Value, String> {
        let mut client = self.connect()?;
        client.call(op, args).map_err(|e| match e {
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

    fn invite(
        &self,
        node: &str,
        name: &str,
        can_be_master: bool,
        acting: bool,
    ) -> Result<Option<String>, String> {
        let args = json!({
            "node": node,
            "name": name,
            "canBeMaster": can_be_master,
            "acting": acting,
        });
        let mut client = self.connect()?;
        invite_answer(client.call(ops::CLUSTER_INVITE, args))
    }

    fn primary_addr(&self) -> Option<(String, u16)> {
        primary_addr_in(&self.recent_status()?)
    }

    fn tailnet(&self) -> Option<String> {
        let mut cached = self.tailnet.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, name)) = cached.as_ref() {
            if at.elapsed() < TAILNET_TTL {
                return name.clone();
            }
        }
        let name = (self.tailscale)().and_then(|text| tailnet_in(&text));
        *cached = Some((Instant::now(), name.clone()));
        name
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

pub fn invite_answer(reply: Result<Value, ClientError>) -> Result<Option<String>, String> {
    match reply {
        Ok(value) => token_in(&value)
            .map(Some)
            .ok_or_else(|| "the cluster gave no invite".to_string()),
        Err(ClientError::Api(api)) if api.code == ErrorCode::Forbidden => Ok(None),
        Err(ClientError::Api(api)) => Err(api.message),
        Err(ClientError::Protocol(why)) => Err(why),
        Err(_) => Err(NOT_RUNNING.to_string()),
    }
}

pub fn primary_addr_in(status: &Value) -> Option<(String, u16)> {
    let home = status.get("cluster")?.get("home")?.as_str()?;
    let member = status
        .get("members")?
        .as_array()?
        .iter()
        .find(|m| m.get("id").and_then(Value::as_str) == Some(home))?;
    let addr = member
        .get("addrs")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find(|ip| is_tailscale_ip(ip))?;
    let port = member
        .get("port")?
        .as_u64()
        .filter(|p| (1..=u64::from(u16::MAX)).contains(p))?;
    Some((addr.to_string(), port as u16))
}

pub fn tailnet_in(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let value = serde_json::Deserializer::from_str(&text[start..])
        .into_iter::<Value>()
        .next()?
        .ok()?;
    let name = value.get("CurrentTailnet")?.get("Name")?.as_str()?.trim();
    let plain = !name.is_empty()
        && name.len() <= TAILNET_NAME_MAX
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@' | '+'));
    plain.then(|| name.to_string())
}

fn run_tailscale() -> Option<String> {
    let cli = TAILSCALE_CLI
        .iter()
        .find(|p| Path::new(p).exists())
        .copied()
        .unwrap_or("tailscale");
    let argv = [cli, "status", "--json", "--peers=false"]
        .map(str::to_string)
        .to_vec();
    let ran = ProcessRunner.run(&Call::secs(argv, 5));
    (ran.code == 0).then_some(ran.out)
}
