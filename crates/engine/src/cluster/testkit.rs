use std::collections::BTreeMap;

use super::id::{fingerprint, NodeId};
use super::members::{Desired, Member, MemberState};
use super::versions::{Versions, CLUSTER_SCHEMA, FEATURE_DISPATCH, NODE_PROTO};
use crate::versions::{Tool, Version};

pub fn fp(name: &str) -> String {
    fingerprint(name.as_bytes())
}

pub fn id(name: &str) -> NodeId {
    NodeId::from_fingerprint(&fp(name)).expect("a node id")
}

pub fn versions(os: &str) -> Versions {
    let mut tools = BTreeMap::new();
    tools.insert(Tool::Claude, Version::parse("2.1.324").expect("version"));
    tools.insert(Tool::Codex, Version::parse("0.166.0").expect("version"));
    tools.insert(Tool::Agy, Version::parse("1.4.2").expect("version"));
    Versions {
        notchling: "0.1.0".to_string(),
        proto: NODE_PROTO,
        features: vec![FEATURE_DISPATCH.to_string()],
        schema: CLUSTER_SCHEMA,
        os: os.to_string(),
        arch: "arm64".to_string(),
        tools,
        logins: BTreeMap::new(),
    }
}

pub fn member(name: &str, os: &str, client: bool) -> Member {
    Member {
        id: id(name),
        name: name.to_string(),
        fingerprint: fp(name),
        addrs: vec![format!("100.64.0.{}", name.len())],
        port: 47823,
        client,
        can_be_master: false,
        reference: false,
        accepts_delegation: false,
        state: MemberState::Active,
        versions: Some(versions(os)),
        desired: Desired::default(),
        joined_ms: 0,
    }
}
