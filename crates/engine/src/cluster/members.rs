use std::fmt;

use serde::{Deserialize, Serialize};

use super::id::NodeId;
use super::versions::Versions;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Term {
    pub n: u64,
    pub node: NodeId,
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.n, self.node)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemberState {
    Pending,
    Active,
    Removed,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Desired {
    pub acting: bool,
    pub bridge: bool,
    pub install: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: NodeId,
    pub name: String,
    pub fingerprint: String,
    pub addrs: Vec<String>,
    pub port: u16,
    pub client: bool,
    pub can_be_master: bool,
    pub reference: bool,
    pub accepts_delegation: bool,
    #[serde(default)]
    pub headless: bool,
    pub state: MemberState,
    pub versions: Option<Versions>,
    pub desired: Desired,
    pub joined_ms: i64,
}

pub fn can_be_master_default(os: &str) -> bool {
    os == "macos"
}
