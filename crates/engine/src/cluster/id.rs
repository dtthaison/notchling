use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::versions;

const ID_LEN: usize = 16;
const FINGERPRINT_LEN: usize = 64;

fn lower_hex(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct NodeId(String);

impl NodeId {
    pub fn from_fingerprint(fp: &str) -> Option<NodeId> {
        if fp.len() != FINGERPRINT_LEN || !lower_hex(fp) {
            return None;
        }
        Some(NodeId(fp[..ID_LEN].to_string()))
    }

    pub fn parse(s: &str) -> Option<NodeId> {
        if s.len() != ID_LEN || !lower_hex(s) {
            return None;
        }
        Some(NodeId(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NodeId {
    type Error = String;

    fn try_from(s: String) -> Result<NodeId, String> {
        NodeId::parse(&s).ok_or_else(|| "not a node id".to_string())
    }
}

impl From<NodeId> for String {
    fn from(id: NodeId) -> String {
        id.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn fingerprint(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    let mut out = String::with_capacity(FINGERPRINT_LEN);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Qualified {
    pub node: NodeId,
    pub id: String,
}

impl Qualified {
    pub fn parse(s: &str) -> Option<Qualified> {
        let (node, id) = s.split_once('/')?;
        if id.is_empty() {
            return None;
        }
        Some(Qualified {
            node: NodeId::parse(node)?,
            id: id.to_string(),
        })
    }
}

pub fn qualify(node: &NodeId, id: &str) -> String {
    format!("{node}/{id}")
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Home,
    Standby,
    Worker,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Claude,
    Codex,
    Agy,
    Shell,
}

impl Tool {
    pub fn title(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Agy => "Antigravity",
            Tool::Shell => "Shell",
        }
    }
}

pub fn tool_of(t: Tool) -> Option<versions::Tool> {
    match t {
        Tool::Claude => Some(versions::Tool::Claude),
        Tool::Codex => Some(versions::Tool::Codex),
        Tool::Agy => Some(versions::Tool::Agy),
        Tool::Shell => None,
    }
}
