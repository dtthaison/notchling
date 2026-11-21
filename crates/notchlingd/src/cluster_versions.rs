use notchling_engine::cluster::build_id::build_id;
use notchling_engine::cluster::versions::{
    CLUSTER_SCHEMA, FEATURE_DISPATCH, FEATURE_INBOX_CONSENT, NODE_PROTO,
};
use notchling_engine::cluster::Versions;
use notchling_engine::versions::mac::{detect, Roots};
use notchling_engine::versions::{IdentityCache, Tool, Version};
use std::collections::BTreeMap;

pub fn own() -> Versions {
    Versions {
        notchling: env!("CARGO_PKG_VERSION").to_string(),
        proto: NODE_PROTO,
        features: vec![
            FEATURE_DISPATCH.to_string(),
            FEATURE_INBOX_CONSENT.to_string(),
        ],
        schema: CLUSTER_SCHEMA,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        tools: BTreeMap::new(),
        logins: BTreeMap::new(),
        own_logins: BTreeMap::new(),
        host: None,
        bridge: None,
        build: build_id().map(str::to_owned),
    }
}

pub const NODE_TOOLS: [Tool; 3] = [Tool::Claude, Tool::Codex, Tool::Agy];

pub fn with_tools(tools: BTreeMap<Tool, Version>) -> Versions {
    let mut versions = own();
    versions.tools = tools
        .into_iter()
        .filter(|(tool, _)| NODE_TOOLS.contains(tool))
        .collect();
    versions
}

pub fn with_logins(mut versions: Versions, claude: bool) -> Versions {
    if claude {
        versions.logins.insert(Tool::Claude, true);
    }
    versions
}

pub fn with_probed(mut versions: Versions, probed: BTreeMap<Tool, bool>) -> Versions {
    for (tool, signed_in) in probed {
        versions.logins.entry(tool).or_insert(signed_in);
    }
    versions
}

pub fn with_own(mut versions: Versions, own: BTreeMap<Tool, bool>) -> Versions {
    versions.own_logins.extend(own);
    versions
}

pub fn probe(roots: &Roots, cache: &mut IdentityCache, agy: Option<Version>) -> Versions {
    let mut tools = BTreeMap::new();
    for tool in [Tool::Claude, Tool::Codex] {
        if let Some(version) = detect(roots, tool, cache).and_then(|i| i.version) {
            tools.insert(tool, version);
        }
    }
    if let Some(agy) = agy {
        tools.insert(Tool::Agy, agy);
    }
    with_tools(tools)
}

#[cfg(test)]
#[path = "cluster_versions_tests.rs"]
mod tests;
