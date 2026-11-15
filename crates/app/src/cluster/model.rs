use notchling_engine::cluster::{compat, Compat, Desired, Purpose, Tool as ClusterTool, Versions};
use notchling_engine::versions::Tool as VersionTool;
use notchling_locale as t;
use serde_json::Value;

use super::wire::{self, Request};

const TOOLS: [(VersionTool, ClusterTool); 3] = [
    (VersionTool::Claude, ClusterTool::Claude),
    (VersionTool::Codex, ClusterTool::Codex),
    (VersionTool::Agy, ClusterTool::Agy),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRow {
    pub id: String,
    pub name: String,
    pub role: &'static str,
    pub can_be_master: bool,
    pub online: bool,
    pub os: String,
    pub app_version: Option<String>,
    pub cpu: Option<u8>,
    pub mem: Option<(u64, u64)>,
    pub tools: Vec<(String, Option<String>)>,
    pub warnings: Vec<String>,
    pub desired: (bool, bool, bool),
    pub me: bool,
    pub pending: bool,
    pub removed: bool,
    pub active: bool,
    pub primary: bool,
    pub client: bool,
}

impl NodeRow {
    pub fn offers_menu(&self) -> bool {
        self.active
    }

    pub fn offers_acting(&self) -> bool {
        self.offers_menu() && !self.me && !self.client
    }

    pub fn offers_remove(&self) -> bool {
        self.offers_menu() && !self.me && !self.primary
    }

    pub fn state_word(&self) -> &'static str {
        if self.removed {
            t::cluster::removed()
        } else if self.pending {
            t::cluster::pending()
        } else if self.online {
            t::cluster::online()
        } else {
            t::cluster::offline()
        }
    }

    pub fn load_text(&self) -> Option<String> {
        let cpu = self.cpu?;
        let (used, total) = self.mem?;
        Some(t::cluster::load(
            &cpu.to_string(),
            &gigabytes(used),
            &gigabytes(total),
        ))
    }

    pub fn tool_texts(&self) -> Vec<String> {
        self.tools
            .iter()
            .map(|(tool, version)| match version {
                Some(version) => t::cluster::tool(tool, version),
                None => t::cluster::tool_missing(tool),
            })
            .collect()
    }
}

fn gigabytes(mb: u64) -> String {
    let tenths = (mb * 10 + 512) / 1024;
    if tenths.is_multiple_of(10) {
        (tenths / 10).to_string()
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Off,
    NoTailscale,
    NotListening,
    Connected,
    Error(String),
}

impl Line {
    pub fn text(&self) -> String {
        match self {
            Line::Off => t::cluster::link_off().to_owned(),
            Line::NoTailscale => t::cluster::link_no_tailscale().to_owned(),
            Line::NotListening => t::cluster::link_not_listening().to_owned(),
            Line::Connected => t::cluster::link_on().to_owned(),
            Line::Error(error) => error.clone(),
        }
    }

    pub fn is_problem(&self) -> bool {
        !matches!(self, Line::Connected)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailoverStatus {
    pub value: String,
    pub armed: bool,
    pub target: Option<String>,
}

fn failover_of(status: &Value) -> Option<FailoverStatus> {
    let failover = status.get("failover").filter(|f| f.is_object())?;
    Some(FailoverStatus {
        value: text(failover, "value").to_owned(),
        armed: flag(failover, "armed"),
        target: failover
            .get("target")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClusterModel {
    loaded: bool,
    cluster: Option<String>,
    failover: Option<FailoverStatus>,
    me: String,
    rows: Vec<NodeRow>,
    link: bool,
    listening: bool,
    tailscale: bool,
    error: Option<String>,
}

fn role_word(role: &str) -> &'static str {
    match role {
        "home" => t::cluster::role_primary(),
        "standby" => t::cluster::role_standby(),
        "worker" => t::cluster::role_worker(),
        _ => t::cluster::role_unknown(),
    }
}

fn role_rank(role: &str) -> u8 {
    match role {
        "home" => 0,
        "standby" => 1,
        "worker" => 2,
        _ => 3,
    }
}

fn versions_of(member: &Value) -> Option<Versions> {
    serde_json::from_value(member.get("versions")?.clone()).ok()
}

fn os_word(versions: &Versions) -> String {
    let os = match versions.os.as_str() {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    };
    if versions.arch.is_empty() {
        os.to_owned()
    } else {
        format!("{os} {}", versions.arch)
    }
}

fn warnings_for(reference: &Versions, node: &Versions, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Compat::Refuse(text) | Compat::Warn(text) = compat(reference, node, name, Purpose::Link)
    {
        out.push(text);
        return out;
    }
    let mut add = |text: String| {
        for part in text.split("; ") {
            if !out.iter().any(|seen| seen == part) {
                out.push(part.to_owned());
            }
        }
    };
    if let Compat::Refuse(text) | Compat::Warn(text) =
        compat(reference, node, name, Purpose::Dispatch(ClusterTool::Shell))
    {
        add(text);
    }
    for (_, tool) in TOOLS {
        if let Compat::Warn(text) = compat(reference, node, name, Purpose::Dispatch(tool)) {
            add(text);
        }
    }
    out
}

fn flag(member: &Value, key: &str) -> bool {
    member.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn text<'a>(member: &'a Value, key: &str) -> &'a str {
    member.get(key).and_then(Value::as_str).unwrap_or("")
}

fn row_of(member: &Value, me: &str, reference: Option<&Versions>) -> NodeRow {
    let id = text(member, "id").to_owned();
    let name = text(member, "name").to_owned();
    let versions = versions_of(member);
    let load = member.get("load").filter(|l| l.is_object());
    let number = |key: &str| load.and_then(|l| l.get(key)).and_then(Value::as_u64);
    let desired = member.get("desired").cloned().unwrap_or(Value::Null);
    let desired_flag = |key: &str| flag(&desired, key);
    let warnings = match (reference, versions.as_ref()) {
        (Some(reference), Some(node)) if !flag(member, "reference") => {
            warnings_for(reference, node, &name)
        }
        _ => Vec::new(),
    };
    let tools = versions
        .as_ref()
        .map(|v| {
            TOOLS
                .iter()
                .map(|(tool, _)| {
                    (
                        tool.title().to_owned(),
                        v.tools.get(tool).map(|version| version.to_string()),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let state = text(member, "state");
    NodeRow {
        me: id == me,
        id,
        name,
        role: role_word(text(member, "role")),
        can_be_master: flag(member, "canBeMaster"),
        online: flag(member, "online"),
        os: versions.as_ref().map(os_word).unwrap_or_default(),
        app_version: versions.as_ref().map(|v| v.notchling.clone()),
        cpu: number("cpu").map(|c| c.min(100) as u8),
        mem: number("memUsedMb").zip(number("memTotalMb")),
        tools,
        warnings,
        desired: (
            desired_flag("acting"),
            desired_flag("bridge"),
            desired_flag("install"),
        ),
        pending: state == "pending",
        removed: state == "removed",
        active: state == "active",
        primary: text(member, "role") == "home",
        client: flag(member, "client"),
    }
}

impl ClusterModel {
    pub fn from_status(status: &Value, me: &str) -> ClusterModel {
        let members: Vec<&Value> = status
            .get("members")
            .and_then(Value::as_array)
            .map(|m| m.iter().collect())
            .unwrap_or_default();
        let reference = members
            .iter()
            .find(|m| flag(m, "reference") && text(m, "state") == "active")
            .and_then(|m| versions_of(m));
        let mut ranked: Vec<(u8, bool, &Value)> = members
            .iter()
            .map(|m| {
                (
                    role_rank(text(m, "role")),
                    text(m, "state") == "removed",
                    *m,
                )
            })
            .collect();
        ranked.sort_by(|a, b| (a.1, a.0, text(a.2, "name")).cmp(&(b.1, b.0, text(b.2, "name"))));
        let cluster = status
            .get("cluster")
            .filter(|c| c.is_object())
            .map(|c| text(c, "name").to_owned());
        ClusterModel {
            loaded: true,
            cluster,
            failover: failover_of(status),
            me: me.to_owned(),
            rows: ranked
                .into_iter()
                .map(|(_, _, m)| row_of(m, me, reference.as_ref()))
                .collect(),
            link: flag(status, "link"),
            listening: status
                .get("listening")
                .and_then(Value::as_array)
                .is_some_and(|ports| !ports.is_empty()),
            tailscale: flag(status, "tailscale"),
            error: status
                .get("error")
                .and_then(Value::as_str)
                .filter(|e| !e.is_empty())
                .map(str::to_owned),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn cluster_name(&self) -> Option<&str> {
        self.cluster.as_deref()
    }

    pub fn failover(&self) -> Option<&FailoverStatus> {
        self.failover.as_ref()
    }

    pub fn rows(&self) -> Vec<NodeRow> {
        self.rows.clone()
    }

    pub fn row(&self, id: &str) -> Option<&NodeRow> {
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn me(&self) -> &str {
        &self.me
    }

    pub fn can_create(&self) -> bool {
        self.loaded && self.cluster.is_none() && self.error.is_none()
    }

    pub fn unbuilt_error(&self) -> Option<&str> {
        self.cluster
            .is_none()
            .then_some(self.error.as_deref())
            .flatten()
    }

    pub fn can_copy_token(&self) -> bool {
        self.cluster.is_some()
    }

    pub fn line(&self) -> Line {
        if let Some(error) = &self.error {
            Line::Error(error.clone())
        } else if !self.link {
            Line::Off
        } else if !self.tailscale {
            Line::NoTailscale
        } else if !self.listening {
            Line::NotListening
        } else {
            Line::Connected
        }
    }

    pub fn can_turn_on(&self) -> bool {
        self.cluster.is_some() && !self.link
    }

    pub fn toggle_can_be_master(&self, id: &str) -> Option<Request> {
        let row = self.row(id).filter(|r| r.offers_menu())?;
        Some(wire::can_be_master(&row.id, !row.can_be_master))
    }

    pub fn toggle_acting(&self, id: &str) -> Option<Request> {
        let row = self.row(id).filter(|r| r.offers_acting())?;
        let (acting, bridge, install) = row.desired;
        Some(wire::desired(
            &row.id,
            Desired {
                acting: !acting,
                bridge,
                install,
            },
        ))
    }

    pub fn remove(&self, id: &str) -> Option<Request> {
        let row = self.row(id).filter(|r| r.offers_remove())?;
        Some(wire::remove(&row.id))
    }
}
