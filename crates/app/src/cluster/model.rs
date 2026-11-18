use notchling_engine::cluster::grants::SecretAllow;
use notchling_engine::cluster::{compat, Compat, Desired, Purpose, Tool as ClusterTool, Versions};
use notchling_engine::versions::{mismatch_between, Tool as VersionTool, Version};
use notchling_locale as t;
use serde_json::Value;

use super::move_sheet::MoveStatus;
use super::promote::PromoteInfo;
use super::replication::ReplicationRow;
use super::secrets::SecretsModel;
use super::wire::{self, Request};
use crate::tools::requests::RowTool;
use notchling_engine::cluster::Qualified;
use std::collections::{BTreeMap, BTreeSet};

const TOOLS: [(VersionTool, ClusterTool); 3] = [
    (VersionTool::Claude, ClusterTool::Claude),
    (VersionTool::Codex, ClusterTool::Codex),
    (VersionTool::Agy, ClusterTool::Agy),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Login {
    SignedIn,
    SignedOut,
    NotInstalled,
    Unknown,
}

impl Login {
    pub fn word(self) -> &'static str {
        match self {
            Login::SignedIn => t::cluster::login_signed_in(),
            Login::SignedOut => t::cluster::login_signed_out(),
            Login::NotInstalled => t::cluster::login_not_installed(),
            Login::Unknown => t::cluster::login_unknown(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultNode {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub primary: bool,
    pub me: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDrift {
    pub tool: VersionTool,
    pub found: Version,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRow {
    pub id: String,
    pub name: String,
    pub role: &'static str,
    pub can_be_master: bool,
    pub online: bool,
    pub os: String,
    pub app_version: Option<String>,
    pub build: Option<String>,
    pub cpu: Option<u8>,
    pub mem: Option<(u64, u64)>,
    pub tools: Vec<(String, Option<String>)>,
    pub versions: Option<Versions>,
    pub logins: Vec<(String, Login)>,
    pub worker_login: Option<bool>,
    pub secrets: SecretAllow,
    pub warnings: Vec<String>,
    pub drifts: Vec<ToolDrift>,
    pub desired: (bool, bool, bool),
    pub me: bool,
    pub pending: bool,
    pub removed: bool,
    pub active: bool,
    pub primary: bool,
    pub client: bool,
    pub headless: bool,
    pub replication: Option<ReplicationRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeepOffer {
    pub machine: String,
    pub kept: bool,
}

impl NodeRow {
    pub fn offers_menu(&self) -> bool {
        self.active
    }

    pub fn offers_acting(&self) -> bool {
        self.offers_menu() && !self.me && !self.client
    }

    pub fn offers_install_hook(&self) -> bool {
        self.offers_menu() && self.online && !self.me && self.headless
    }

    pub fn offers_secrets(&self) -> bool {
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

    pub fn login_texts(&self) -> Vec<(String, Login)> {
        self.logins
            .iter()
            .map(|(tool, login)| (t::cluster::login(tool, login.word()), *login))
            .collect()
    }

    pub fn worker_login_text(&self) -> Option<(String, bool)> {
        let on = self.worker_login.filter(|_| self.offers_secrets())?;
        let word = if on {
            t::cluster::worker_login_on()
        } else {
            t::cluster::worker_login_off()
        };
        Some((t::cluster::worker_login(word), on))
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClusterModel {
    loaded: bool,
    cluster: Option<String>,
    joining: Option<bool>,
    pub(super) promote: PromoteInfo,
    me: String,
    rows: Vec<NodeRow>,
    reference: Option<Versions>,
    main: BTreeSet<String>,
    kept: BTreeSet<String>,
    shell_jobs: BTreeMap<String, usize>,
    live_jobs: BTreeMap<String, usize>,
    moving: Option<MoveStatus>,
    link: bool,
    listening: bool,
    tailscale: bool,
    error: Option<String>,
    vault: Option<VaultNode>,
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
        "macos" => t::cluster::os_macos(),
        "linux" => t::cluster::os_linux(),
        "windows" => t::cluster::os_windows(),
        other => other,
    };
    if versions.arch.is_empty() {
        os.to_owned()
    } else {
        t::cluster::os_arch(os, &versions.arch)
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

fn drifts_for(reference: &Versions, node: &Versions, name: &str) -> Vec<ToolDrift> {
    let keep = |tools: &BTreeMap<VersionTool, Version>| -> BTreeMap<VersionTool, Version> {
        tools
            .iter()
            .filter(|(tool, _)| TOOLS.iter().any(|(t, _)| t == *tool))
            .map(|(tool, version)| (*tool, version.clone()))
            .collect()
    };
    mismatch_between(&keep(&reference.tools), name, &keep(&node.tools))
        .into_iter()
        .map(|m| ToolDrift {
            tool: m.tool,
            text: m.text(),
            found: m.worker,
        })
        .collect()
}

fn login_of(versions: &Versions, tool: VersionTool) -> Login {
    if !versions.tools.contains_key(&tool) {
        return Login::NotInstalled;
    }
    let own = versions.own_logins.get(&tool).copied();
    match (versions.logins.get(&tool).copied(), own) {
        (Some(true), _) | (_, Some(true)) => Login::SignedIn,
        (Some(false), _) | (_, Some(false)) => Login::SignedOut,
        (None, None) => Login::Unknown,
    }
}

fn worker_login_of(versions: &Versions) -> Option<bool> {
    versions
        .tools
        .contains_key(&VersionTool::Claude)
        .then(|| versions.logins.get(&VersionTool::Claude) == Some(&true))
}

fn secrets_of(member: &Value) -> SecretAllow {
    member
        .get("secrets")
        .and_then(|s| serde_json::from_value(s.clone()).ok())
        .unwrap_or_default()
}

fn vault_of(cluster: &Value, members: &[&Value], me: &str) -> Option<VaultNode> {
    let id = text(cluster, "vault");
    let member = members.iter().find(|m| text(m, "id") == id)?;
    Some(VaultNode {
        id: id.to_owned(),
        name: text(member, "name").to_owned(),
        online: id == me || flag(member, "online"),
        primary: text(cluster, "home") == id,
        me: id == me,
    })
}

fn flag(member: &Value, key: &str) -> bool {
    member.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn text<'a>(member: &'a Value, key: &str) -> &'a str {
    member.get(key).and_then(Value::as_str).unwrap_or("")
}

fn row_of(member: &Value, status: &Value, me: &str, reference: Option<&Versions>) -> NodeRow {
    let id = text(member, "id").to_owned();
    let id_text = id.clone();
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
    let drifts = match (reference, versions.as_ref()) {
        (Some(reference), Some(node)) if !flag(member, "reference") => {
            drifts_for(reference, node, &name)
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
    let logins = versions
        .as_ref()
        .map(|v| {
            TOOLS
                .iter()
                .map(|(tool, _)| (tool.title().to_owned(), login_of(v, *tool)))
                .collect()
        })
        .unwrap_or_default();
    let worker_login = versions.as_ref().and_then(worker_login_of);
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
        build: versions.as_ref().and_then(|v| v.build.clone()),
        cpu: number("cpu").map(|c| c.min(100) as u8),
        mem: number("memUsedMb").zip(number("memTotalMb")),
        tools,
        versions,
        logins,
        worker_login,
        secrets: secrets_of(member),
        warnings,
        drifts,
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
        headless: flag(member, "headless"),
        replication: ReplicationRow::offered(member)
            .then(|| ReplicationRow::from(status, &id_text)),
    }
}

fn id_set(status: &Value, key: &str) -> BTreeSet<String> {
    status
        .get(key)
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn shell_jobs(status: &Value) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let jobs = status.get("jobs").and_then(Value::as_array);
    for job in jobs.into_iter().flatten() {
        let live = matches!(text(job, "state"), "running" | "dispatching");
        if live && text(job, "tool") == "shell" {
            *counts.entry(text(job, "node").to_owned()).or_insert(0) += 1;
        }
    }
    counts
}

fn live_jobs(status: &Value) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let jobs = status.get("jobs").and_then(Value::as_array);
    for job in jobs.into_iter().flatten() {
        if matches!(text(job, "state"), "running" | "dispatching") {
            *counts.entry(text(job, "node").to_owned()).or_insert(0) += 1;
        }
    }
    counts
}

impl ClusterModel {
    pub fn running_jobs(&self, node: &str) -> usize {
        self.live_jobs.get(node).copied().unwrap_or(0)
    }

    pub fn running_shell_jobs(&self, node: &str) -> usize {
        self.shell_jobs.get(node).copied().unwrap_or(0)
    }

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
        let joining = status
            .get("joining")
            .filter(|j| j.is_object() && cluster.is_none())
            .map(|j| flag(j, "answered"));
        let vault = status
            .get("cluster")
            .filter(|c| c.is_object())
            .and_then(|c| vault_of(c, &members, me));
        ClusterModel {
            vault,
            loaded: true,
            cluster,
            joining,
            promote: PromoteInfo::of(status),
            me: me.to_owned(),
            rows: ranked
                .into_iter()
                .map(|(_, _, m)| row_of(m, status, me, reference.as_ref()))
                .collect(),
            reference,
            main: id_set(status, "main"),
            kept: id_set(status, "kept"),
            shell_jobs: shell_jobs(status),
            live_jobs: live_jobs(status),
            moving: MoveStatus::of(status),
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

    pub fn rows(&self) -> Vec<NodeRow> {
        self.rows.clone()
    }

    pub fn move_status(&self) -> Option<&MoveStatus> {
        self.moving.as_ref()
    }

    pub fn link_on(&self) -> bool {
        self.link
    }

    pub fn move_running(&self) -> bool {
        self.moving.as_ref().is_some_and(MoveStatus::running)
    }

    pub fn primary_row(&self) -> Option<&NodeRow> {
        self.rows.iter().find(|r| r.primary && r.active)
    }

    pub fn this_is_primary(&self) -> bool {
        self.cluster.is_some() && self.row(&self.me).is_some_and(|r| r.primary && r.active)
    }

    pub fn offers_move_now(&self, id: &str) -> bool {
        self.cluster.is_some()
            && self.link
            && !self.move_running()
            && self.primary_row().is_some_and(|p| p.online)
            && self.row(id).is_some_and(|r| {
                r.active
                    && r.online
                    && r.can_be_master
                    && !r.primary
                    && r.replication.as_ref().is_some_and(|m| m.ready.is_ok())
            })
    }

    pub fn move_home(&self, id: &str) -> Option<Request> {
        self.offers_move_now(id).then(|| wire::move_home(id))
    }

    pub fn move_now(&self) -> Option<Request> {
        self.move_running().then(wire::move_now)
    }

    pub fn keep_offer(&self, session: &str) -> Option<KeepOffer> {
        if !self.this_is_primary() || Qualified::parse(session).is_some() {
            return None;
        }
        if RowTool::of(session) == RowTool::Agy {
            return None;
        }
        let kept = self.kept.contains(session);
        if !kept && !self.main.contains(session) {
            return None;
        }
        Some(KeepOffer {
            machine: self.row(&self.me)?.name.clone(),
            kept,
        })
    }

    pub fn keep_toggle(&self, session: &str) -> Option<Request> {
        let offer = self.keep_offer(session)?;
        Some(wire::main_set(session, offer.kept))
    }

    pub fn row(&self, id: &str) -> Option<&NodeRow> {
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn me(&self) -> &str {
        &self.me
    }

    pub fn reference(&self) -> Option<&Versions> {
        self.reference.as_ref()
    }

    pub fn vault_node(&self) -> Option<&VaultNode> {
        self.vault.as_ref()
    }

    pub fn remote_vault(&self) -> Option<&VaultNode> {
        self.vault.as_ref().filter(|v| !v.me)
    }

    pub fn can_create(&self) -> bool {
        self.loaded && self.cluster.is_none() && self.joining.is_none() && self.error.is_none()
    }

    pub fn can_join(&self) -> bool {
        self.can_create()
    }

    pub fn is_joining(&self) -> bool {
        self.joining.is_some()
    }

    pub fn joining_text(&self) -> Option<&'static str> {
        self.joining.map(|answered| {
            if answered {
                t::cluster::joining_waiting()
            } else {
                t::cluster::joining_asking()
            }
        })
    }

    pub fn unbuilt_error(&self) -> Option<&str> {
        self.cluster
            .is_none()
            .then_some(self.error.as_deref())
            .flatten()
    }

    pub fn can_copy_token(&self) -> bool {
        self.cluster.is_some() && self.line() == Line::Connected
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

    pub fn off_hint(&self) -> Option<&'static str> {
        (self.can_turn_on() && self.error.is_none()).then(t::cluster::off_hint)
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

    pub fn toggle_install(&self, id: &str) -> Option<Request> {
        let row = self.row(id).filter(|r| r.offers_acting())?;
        let (acting, bridge, install) = row.desired;
        Some(wire::desired(
            &row.id,
            Desired {
                acting,
                bridge,
                install: !install,
            },
        ))
    }

    fn installable(&self, id: &str) -> Option<&NodeRow> {
        let primary = self.row(&self.me).is_some_and(|r| r.primary);
        self.row(id).filter(|r| primary && r.offers_install_hook())
    }

    pub fn install_plan(&self, id: &str) -> Option<Request> {
        self.installable(id).map(|r| wire::install_plan(&r.id))
    }

    pub fn install_apply(&self, id: &str) -> Option<Request> {
        self.installable(id).map(|r| wire::install_apply(&r.id))
    }

    pub fn set_secrets(&self, id: &str, secrets: &SecretsModel) -> Option<Request> {
        let row = self.row(id).filter(|r| r.offers_secrets())?;
        Some(wire::allow_set(&row.id, &secrets.request()))
    }

    pub fn remove(&self, id: &str) -> Option<Request> {
        let row = self.row(id).filter(|r| r.offers_remove())?;
        Some(wire::remove(&row.id))
    }
}
