use std::collections::BTreeMap;
use std::path::PathBuf;

use notchling_engine::versions::{Method, Tool, Version};
use notchling_local_api::{ops, Client, ClientError, ClientKind, ConnectOptions, CALL_TIMEOUT};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacTool {
    pub installed: Option<Version>,
    pub method: Method,
    pub reference: Option<Version>,
    pub usable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MacSide {
    pub tools: BTreeMap<Tool, MacTool>,
}

impl MacSide {
    pub fn compare_with(&self) -> BTreeMap<Tool, Version> {
        self.tools
            .iter()
            .filter_map(|(tool, t)| {
                let version = if t.usable {
                    t.reference.clone()
                } else {
                    t.installed.clone()
                };
                version.map(|v| (*tool, v))
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MacVersionsError {
    NotRunning,
    Bad(String),
}

pub trait MacVersions: Send + Sync {
    fn get(&self) -> Result<MacSide, MacVersionsError>;
}

pub struct DaemonVersions {
    pub socket: PathBuf,
}

impl MacVersions for DaemonVersions {
    fn get(&self) -> Result<MacSide, MacVersionsError> {
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
        .map_err(|_| MacVersionsError::NotRunning)?;
        match client.call(ops::VERSIONS_GET, json!({})) {
            Ok(reply) => mac_side(&reply),
            Err(ClientError::Api(e)) => Err(MacVersionsError::Bad(e.message)),
            Err(ClientError::Protocol(e)) => Err(MacVersionsError::Bad(e)),
            Err(_) => Err(MacVersionsError::NotRunning),
        }
    }
}

pub struct FixedVersions(pub Result<MacSide, MacVersionsError>);

impl MacVersions for FixedVersions {
    fn get(&self) -> Result<MacSide, MacVersionsError> {
        self.0.clone()
    }
}

fn version_of(value: Option<&Value>) -> Option<Version> {
    value.and_then(Value::as_str).and_then(Version::parse)
}

pub fn mac_side(reply: &Value) -> Result<MacSide, MacVersionsError> {
    let rows = reply
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| MacVersionsError::Bad("the reply has no tools".into()))?;
    let mut tools = BTreeMap::new();
    for row in rows {
        let Some(tool) = row
            .get("tool")
            .and_then(Value::as_str)
            .and_then(Tool::from_name)
        else {
            continue;
        };
        let installed = version_of(row.get("installed").and_then(|i| i.get("version")));
        let method = row
            .get("method")
            .cloned()
            .and_then(|m| serde_json::from_value(m).ok())
            .unwrap_or(Method::Unknown);
        let reference = version_of(row.get("reference").and_then(|r| r.get("version")));
        let usable = row.get("usable").and_then(Value::as_bool) == Some(true);
        tools.insert(
            tool,
            MacTool {
                installed,
                method,
                reference,
                usable,
            },
        );
    }
    Ok(MacSide { tools })
}

pub fn fixed_mac() -> MacSide {
    let v = |s: &str| Version::parse(s);
    let mut tools = BTreeMap::new();
    tools.insert(
        Tool::Claude,
        MacTool {
            installed: v("2.1.324"),
            method: Method::Native,
            reference: None,
            usable: false,
        },
    );
    tools.insert(
        Tool::Codex,
        MacTool {
            installed: v("0.166.0"),
            method: Method::Npm,
            reference: None,
            usable: false,
        },
    );
    MacSide { tools }
}
