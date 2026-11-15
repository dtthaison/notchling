use notchling_engine::bridge::net::crypto::{hex, random_bytes};
use notchling_engine::cluster::shell_job::{CODEX_LATER, MAX_COMMAND};
use notchling_engine::cluster::{compat, Compat, Purpose, Tool as ClusterTool, Versions};
use notchling_locale as t;
use serde_json::{json, Value};

use super::new_session::{NewSession, Section, Stage};
use crate::acting::Request;
use crate::cluster::model::ClusterModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remote {
    Claude,
    Agy,
    Shell,
    Codex,
}

impl Remote {
    pub const ALL: [Remote; 4] = [Remote::Claude, Remote::Agy, Remote::Shell, Remote::Codex];

    pub fn of_section(section: Section) -> Remote {
        match section {
            Section::Claude => Remote::Claude,
            Section::Codex => Remote::Codex,
            Section::Antigravity => Remote::Agy,
        }
    }

    pub fn tool(self) -> ClusterTool {
        match self {
            Remote::Claude => ClusterTool::Claude,
            Remote::Agy => ClusterTool::Agy,
            Remote::Shell => ClusterTool::Shell,
            Remote::Codex => ClusterTool::Codex,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Remote::Claude => "claude",
            Remote::Agy => "agy",
            Remote::Shell => "shell",
            Remote::Codex => "codex",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Remote::Claude => t::chat::tool_claude_code(),
            Remote::Agy => t::chat::tool_antigravity(),
            Remote::Shell => t::cluster::remote_shell(),
            Remote::Codex => t::chat::tool_codex(),
        }
    }

    pub fn takes_text(self) -> bool {
        matches!(self, Remote::Agy | Remote::Shell)
    }

    pub fn text_title(self) -> &'static str {
        match self {
            Remote::Shell => t::cluster::command_label(),
            _ => t::cluster::first_message_label(),
        }
    }

    pub fn text_hint(self) -> &'static str {
        match self {
            Remote::Shell => t::cluster::command_placeholder(),
            _ => t::cluster::first_message_placeholder(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Send,
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunTarget {
    pub node: Option<String>,
    pub remote: Remote,
    pub code: Code,
    pub path: String,
    pub text: String,
}

impl Default for RunTarget {
    fn default() -> RunTarget {
        RunTarget {
            node: None,
            remote: Remote::Claude,
            code: Code::Folder,
            path: String::new(),
            text: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOnRow {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub reason: Option<String>,
    pub warning: Option<String>,
    pub this_mac: bool,
}

impl RunOnRow {
    pub fn pickable(&self) -> bool {
        self.reason.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    id: String,
    name: String,
    online: bool,
    versions: Option<Versions>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOn {
    me: String,
    reference: Option<Versions>,
    nodes: Vec<Candidate>,
}

impl RunOn {
    pub fn from_cluster(model: &ClusterModel) -> RunOn {
        RunOn {
            me: model.me().to_owned(),
            reference: model.reference().cloned(),
            nodes: model
                .rows()
                .into_iter()
                .filter(|r| r.active && !r.me && !r.removed && !r.pending)
                .map(|r| Candidate {
                    id: r.id,
                    name: r.name,
                    online: r.online,
                    versions: r.versions,
                })
                .collect(),
        }
    }

    pub fn offered(&self) -> bool {
        !self.nodes.is_empty()
    }

    pub fn rows(&self, tool: ClusterTool) -> Vec<RunOnRow> {
        let mut others: Vec<RunOnRow> = self.nodes.iter().map(|c| self.row(c, tool)).collect();
        others.sort_by(|a, b| (!a.online, &a.name).cmp(&(!b.online, &b.name)));
        let mut rows = vec![RunOnRow {
            id: self.me.clone(),
            name: t::cluster::this_mac().to_owned(),
            online: true,
            reason: None,
            warning: None,
            this_mac: true,
        }];
        rows.extend(others);
        rows
    }

    pub fn row_of(&self, id: &str, tool: ClusterTool) -> Option<RunOnRow> {
        self.rows(tool).into_iter().find(|r| r.id == id)
    }

    fn row(&self, c: &Candidate, tool: ClusterTool) -> RunOnRow {
        let mut row = RunOnRow {
            id: c.id.clone(),
            name: c.name.clone(),
            online: c.online,
            reason: None,
            warning: None,
            this_mac: false,
        };
        if !c.online {
            row.reason = Some(t::cluster::node_offline(&c.name));
        } else if tool == ClusterTool::Codex {
            row.reason = Some(CODEX_LATER.to_owned());
        } else {
            match (&self.reference, &c.versions) {
                (Some(reference), Some(node)) => {
                    match compat(reference, node, &c.name, Purpose::Dispatch(tool)) {
                        Compat::Refuse(text) => row.reason = Some(text),
                        Compat::Warn(text) => row.warning = Some(text),
                        Compat::Ok => {}
                    }
                }
                _ => row.reason = Some(t::cluster::run_on_unread(&c.name)),
            }
        }
        row
    }
}

pub fn fresh_key() -> String {
    let bytes = random_bytes(12).unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_le_bytes()[..12].to_vec())
            .unwrap_or_default()
    });
    format!("ns-{}", hex(&bytes))
}

impl NewSession {
    pub fn with_run_on(mut self, run_on: RunOn) -> NewSession {
        self.run_on = run_on;
        self
    }

    pub fn run_on_rows(&self) -> Vec<RunOnRow> {
        self.run_on.rows(self.target.remote.tool())
    }

    pub fn on_worker(&self) -> bool {
        self.target.node.is_some()
    }

    pub fn node_row(&self) -> Option<RunOnRow> {
        let id = self.target.node.as_deref()?;
        self.run_on.row_of(id, self.target.remote.tool())
    }

    pub fn pick_node(&mut self, id: &str) {
        let me = self.run_on.me.clone();
        let Some(row) = self.run_on.row_of(id, self.target.remote.tool()) else {
            return;
        };
        if !row.pickable() {
            return;
        }
        self.target.node = (id != me).then(|| id.to_owned());
        self.clear_failure();
    }

    pub fn pick_remote(&mut self, remote: Remote) {
        self.target.remote = remote;
        self.clear_failure();
    }

    pub fn pick_code(&mut self, code: Code) {
        self.target.code = code;
        self.clear_failure();
    }

    pub fn set_path(&mut self, path: &str) {
        self.target.path = path.to_owned();
        self.clear_failure();
    }

    pub fn set_text(&mut self, text: &str) {
        self.target.text = text.to_owned();
        self.clear_failure();
    }

    fn clear_failure(&mut self) {
        if matches!(self.stage, Stage::Failed(_) | Stage::Sent(_)) {
            self.stage = Stage::Choosing;
        }
    }

    pub fn remote_folder(&self) -> Option<String> {
        match self.target.code {
            Code::Send => self.folders.get(self.folder?).cloned(),
            Code::Folder => {
                let path = self.target.path.trim();
                (path.starts_with('/') && !path.contains('\0')).then(|| path.to_owned())
            }
        }
    }

    pub fn remote_ready(&self) -> bool {
        let target = &self.target;
        if self.node_row().is_none_or(|row| !row.pickable()) || self.busy() {
            return false;
        }
        if self.remote_folder().is_none() {
            return false;
        }
        if !target.remote.takes_text() {
            return target.remote != Remote::Codex;
        }
        let text = target.text.trim();
        !text.is_empty()
            && (target.remote != Remote::Shell || text.len() <= MAX_COMMAND)
            && !text.contains('\0')
    }

    pub fn dispatch_request(&mut self) -> Option<Request> {
        if !self.remote_ready() {
            return None;
        }
        let node = self.target.node.clone()?;
        let mut args = json!({
            "node": node,
            "tool": self.target.remote.word(),
            "cwd": self.remote_folder()?,
            "key": fresh_key(),
            "code": match self.target.code {
                Code::Send => "send",
                Code::Folder => "folder",
            },
        });
        let text = self.target.text.trim();
        match self.target.remote {
            Remote::Shell => args["command"] = json!(text),
            Remote::Agy => args["text"] = json!(text),
            Remote::Claude | Remote::Codex => {}
        }
        self.stage = Stage::Starting;
        Some(Request::cluster_dispatch(args))
    }

    pub fn dispatched(&mut self, reply: &Value) {
        let warning = reply
            .get("warning")
            .and_then(Value::as_str)
            .filter(|w| !w.is_empty())
            .map(str::to_owned);
        let node = self.node_row().map(|r| r.name).unwrap_or_default();
        self.stage = match warning {
            Some(warning) => Stage::Sent(t::cluster::started_with_warning(&node, &warning)),
            None => Stage::Started,
        };
    }

    pub fn remote_footer(&self) -> String {
        let Some(row) = self.node_row() else {
            return t::chat::sheet_pick().to_owned();
        };
        if let Some(reason) = row.reason {
            return reason;
        }
        match self.remote_folder() {
            Some(folder) => t::cluster::run_on_chosen(
                self.target.remote.title(),
                &row.name,
                &super::new_session::folder_title(&folder, &self.folders),
            ),
            None => t::cluster::run_on_needs_folder(&row.name),
        }
    }
}
