use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::id::NodeId;
use super::members::{Member, Term};
use super::versions::Versions;

pub const MAX_LINE: usize = 1 << 20;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum OriginTier {
    Node,
    Agent,
    App,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub node: NodeId,
    pub tier: OriginTier,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WireError {
    pub code: String,
    pub message: String,
}

impl WireError {
    pub fn new(code: &str, message: impl Into<String>) -> WireError {
        WireError {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Msg {
    Hello {
        node: NodeId,
        #[serde(default)]
        cluster: Option<String>,
        versions: Versions,
        boot_id: String,
        #[serde(default)]
        term: Option<Term>,
    },
    HelloOk {
        node: NodeId,
        cluster: String,
        term: Term,
        home: NodeId,
    },
    Refused {
        reason: String,
        #[serde(default)]
        term: Option<Term>,
        #[serde(default)]
        home: Option<NodeId>,
    },
    Join {
        secret: String,
        name: String,
        fingerprint: String,
        addrs: Vec<String>,
        port: u16,
        versions: Versions,
        #[serde(default)]
        headless: bool,
    },
    JoinPending {
        item: String,
    },
    JoinOk {
        cluster: String,
        #[serde(default)]
        name: String,
        term: Term,
        home: NodeId,
        #[serde(default)]
        vault: Option<NodeId>,
        members: Vec<Member>,
    },
    Members {
        term: Term,
        home: NodeId,
        #[serde(default)]
        vault: Option<NodeId>,
        members: Vec<Member>,
    },
    Call {
        id: u64,
        op: String,
        args: Value,
        origin: Origin,
        term: Term,
    },
    Reply {
        id: u64,
        #[serde(default)]
        ok: Option<Value>,
        #[serde(default)]
        err: Option<WireError>,
    },
    Event {
        topic: String,
        name: String,
        data: Value,
    },
    Ping {
        boot_ms: u64,
    },
    Pong {
        echo: u64,
    },
    #[serde(other)]
    Unknown,
}

pub fn encode(msg: &Msg) -> Result<Vec<u8>, String> {
    let mut line = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    line.push(b'\n');
    if line.len() > MAX_LINE {
        return Err("node message is over 1 MiB".to_string());
    }
    Ok(line)
}

pub fn decode(line: &[u8]) -> Result<Msg, String> {
    if line.len() > MAX_LINE {
        return Err("node message is over 1 MiB".to_string());
    }
    serde_json::from_slice(line.trim_ascii_end()).map_err(|e| e.to_string())
}
