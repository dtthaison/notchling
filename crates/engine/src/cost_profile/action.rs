use super::detect::Detected;
use super::keys::Probe;
use super::manifest::{hash_of, KeyRec, Manifest, Part};
use super::model::{Profile, ReportLine};
use super::render::Features;
use super::resolve::Available;
use crate::catalog::Tool;
use crate::install::ToolRoots;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const CONSENT_RULES: &str = "rules";
pub const CONSENT_PSTACK: &str = "pstack";
pub const CONSENT_ORCHESTRATOR: &str = "orchestrator";
pub const CONSENT_ORCHESTRATOR_NEW: &str = "orchestrator_new";
pub const CONSENT_CODEX_AGENTS: &str = "codex_agents";
pub const CONSENT_CODEX_CONFIG: &str = "codex_config";
pub const CONSENTS: [&str; 6] = [
    CONSENT_RULES,
    CONSENT_PSTACK,
    CONSENT_ORCHESTRATOR,
    CONSENT_ORCHESTRATOR_NEW,
    CONSENT_CODEX_AGENTS,
    CONSENT_CODEX_CONFIG,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Create,
    Update,
    Remove,
    Unchanged,
    Skip,
    Offer,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Create => "create",
            Kind::Update => "update",
            Kind::Remove => "remove",
            Kind::Unchanged => "unchanged",
            Kind::Skip => "skip",
            Kind::Offer => "offer",
        }
    }

    pub fn writes(self) -> bool {
        matches!(self, Kind::Create | Kind::Update | Kind::Remove)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Fresh,
    InStep,
    Outdated,
    Edited,
    Missing,
    NotOurs,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Fresh => "fresh",
            State::InStep => "in_step",
            State::Outdated => "outdated",
            State::Edited => "edited",
            State::Missing => "missing",
            State::NotOurs => "not_ours",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Action {
    pub id: String,
    pub tool: Tool,
    pub path: PathBuf,
    pub part: Part,
    pub kind: Kind,
    pub state: State,
    pub needs: Option<&'static str>,
    pub reason: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub diff: String,
    pub keys: Vec<KeyRec>,
    pub notes: Vec<String>,
    pub block: Option<String>,
    pub requires: Vec<String>,
    pub keep_backup_of: Option<String>,
}

impl Action {
    pub fn new(id: &str, tool: Tool, path: &Path, part: Part) -> Action {
        Action {
            id: id.to_string(),
            tool,
            path: path.to_path_buf(),
            part,
            kind: Kind::Skip,
            state: State::Fresh,
            needs: None,
            reason: None,
            before: None,
            after: None,
            diff: String::new(),
            keys: Vec::new(),
            notes: Vec::new(),
            block: None,
            requires: Vec::new(),
            keep_backup_of: None,
        }
    }

    pub fn skip(mut self, reason: &str) -> Action {
        self.kind = Kind::Skip;
        self.reason = Some(reason.to_string());
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub consent: BTreeSet<String>,
    pub force: BTreeSet<String>,
    pub features: Features,
}

impl Options {
    pub fn consenting(parts: &[&str]) -> Options {
        Options {
            consent: parts.iter().map(|p| p.to_string()).collect(),
            ..Options::default()
        }
    }

    pub fn allows(&self, need: Option<&'static str>) -> bool {
        need.is_none_or(|n| self.consent.contains(n))
    }
}

pub struct Env<'a> {
    pub roots: &'a ToolRoots,
    pub data_dir: &'a Path,
    pub avail: &'a Available,
    pub probe: &'a dyn Probe,
    pub manifest: &'a Manifest,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub profile: Profile,
    pub report: Vec<ReportLine>,
    pub detected: Detected,
}

impl Plan {
    pub fn changes(&self) -> bool {
        self.actions.iter().any(|a| a.kind.writes())
    }

    pub fn get(&self, id: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.id == id)
    }
}

pub fn read_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
        Ok(m) if m.file_type().is_symlink() => Err("it is a link".to_string()),
        Ok(m) if !m.is_file() => Err("it is not a plain file".to_string()),
        Ok(m) if m.len() > 1024 * 1024 => Err("it is too big".to_string()),
        Ok(_) => std::fs::read_to_string(path)
            .map(Some)
            .map_err(|e| e.to_string()),
    }
}

pub fn hash(text: &str) -> String {
    hash_of(text)
}
