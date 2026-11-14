use notchling_engine::cluster::versions::{CLUSTER_SCHEMA, FEATURE_DISPATCH, NODE_PROTO};
use notchling_engine::cluster::Versions;
use notchling_engine::versions::mac::{detect, Roots};
use notchling_engine::versions::{IdentityCache, Tool, Version};
use std::collections::BTreeMap;

pub fn own() -> Versions {
    Versions {
        notchling: env!("CARGO_PKG_VERSION").to_string(),
        proto: NODE_PROTO,
        features: vec![FEATURE_DISPATCH.to_string()],
        schema: CLUSTER_SCHEMA,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        tools: BTreeMap::new(),
        logins: BTreeMap::new(),
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
