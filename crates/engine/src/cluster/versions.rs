use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::id::{tool_of, Tool};
use crate::versions::{self, Version};

pub const NODE_PROTO: u32 = 1;
pub const CLUSTER_SCHEMA: u32 = 1;
pub const LINUX_CLAUDE_PROVEN: bool = false;
pub const FEATURE_DISPATCH: &str = "dispatch";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Versions {
    pub notchling: String,
    pub proto: u32,
    #[serde(default)]
    pub features: Vec<String>,
    pub schema: u32,
    pub os: String,
    pub arch: String,
    #[serde(default)]
    pub tools: BTreeMap<versions::Tool, Version>,
    #[serde(default)]
    pub logins: BTreeMap<versions::Tool, bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Link,
    Dispatch(Tool),
    Promote,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compat {
    Ok,
    Warn(String),
    Refuse(String),
}

pub fn compat(reference: &Versions, node: &Versions, node_name: &str, purpose: Purpose) -> Compat {
    compat_with(reference, node, node_name, purpose, LINUX_CLAUDE_PROVEN)
}

pub fn compat_with(
    reference: &Versions,
    node: &Versions,
    node_name: &str,
    purpose: Purpose,
    linux_claude_proven: bool,
) -> Compat {
    if node.proto != reference.proto {
        return Compat::Refuse(format!(
            "{node_name} speaks node protocol {}, this Mac {}",
            node.proto, reference.proto
        ));
    }
    match purpose {
        Purpose::Link => Compat::Ok,
        Purpose::Dispatch(tool) => dispatch(reference, node, node_name, tool, linux_claude_proven),
        Purpose::Promote => promote(reference, node, node_name),
    }
}

fn dispatch(
    reference: &Versions,
    node: &Versions,
    node_name: &str,
    tool: Tool,
    linux_claude_proven: bool,
) -> Compat {
    if !node.features.iter().any(|f| f == FEATURE_DISPATCH) {
        return Compat::Refuse(format!("{node_name} doesn't take dispatched jobs"));
    }
    if node.schema != reference.schema {
        return Compat::Refuse(schema_text(node_name, node, reference));
    }
    let mut warnings = Vec::new();
    if let Some(vt) = tool_of(tool) {
        if !node.tools.contains_key(&vt) {
            return Compat::Refuse(format!("{} isn't installed on {node_name}", tool.title()));
        }
        if tool == Tool::Claude && node.os == "windows" {
            return Compat::Refuse("Claude Code doesn't run on Windows nodes in v1".to_string());
        }
        if tool == Tool::Claude && node.os == "linux" && !linux_claude_proven {
            return Compat::Refuse("Claude Code on Linux isn't proven yet".to_string());
        }
        let mine: BTreeMap<_, _> = reference
            .tools
            .iter()
            .filter(|(t, _)| **t == vt)
            .map(|(t, v)| (*t, v.clone()))
            .collect();
        let theirs: BTreeMap<_, _> = node
            .tools
            .iter()
            .filter(|(t, _)| **t == vt)
            .map(|(t, v)| (*t, v.clone()))
            .collect();
        for m in versions::mismatch_between(&mine, node_name, &theirs) {
            warnings.push(m.text());
        }
    }
    if node.notchling != reference.notchling {
        warnings.push(notchling_text(node_name, node, reference));
    }
    if warnings.is_empty() {
        Compat::Ok
    } else {
        Compat::Warn(warnings.join("; "))
    }
}

fn promote(reference: &Versions, node: &Versions, node_name: &str) -> Compat {
    if node.notchling != reference.notchling {
        return Compat::Refuse(notchling_text(node_name, node, reference));
    }
    if node.schema != reference.schema {
        return Compat::Refuse(schema_text(node_name, node, reference));
    }
    if let Some(mac) = reference.tools.get(&versions::Tool::Claude) {
        match node.tools.get(&versions::Tool::Claude) {
            None => {
                return Compat::Refuse(format!("Claude Code isn't installed on {node_name}"));
            }
            Some(theirs) if theirs != mac => {
                return Compat::Refuse(format!(
                    "{node_name} runs Claude Code {theirs}, the Mac {mac}"
                ));
            }
            Some(_) => {}
        }
    }
    Compat::Ok
}

fn notchling_text(node_name: &str, node: &Versions, reference: &Versions) -> String {
    format!(
        "{node_name} runs Notchling {}, the Mac {}",
        node.notchling, reference.notchling
    )
}

fn schema_text(node_name: &str, node: &Versions, reference: &Versions) -> String {
    format!(
        "{node_name} has cluster schema {}, this Mac {}",
        node.schema, reference.schema
    )
}
