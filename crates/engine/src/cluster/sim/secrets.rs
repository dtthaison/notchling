use std::collections::{BTreeMap, BTreeSet};

use crate::cluster::core::Effect;
use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use crate::inbox::Book;

pub const MARKER: &str = "NL-MARKER-";
pub const JANITOR_MS: u64 = 30_000;
pub const GRANT_LIFE_MS: u64 = 12 * 3_600_000;
pub const KNOWN_WAIT_MS: u64 = 3_000;
/// 30 s sweep, a 1 s tick and 1 s of margin, from when the Worker can know
/// its job ended (its own process ending).
pub const STEADY_MS: u64 = JANITOR_MS + 2_000;
/// After a restart or a resume the janitor starts with a tick of its own;
/// the bound is its own number, measured from the node coming up.
pub const RECOVERY_MS: u64 = JANITOR_MS + 5_000;
/// The daemon's outbox resends a full sync to a target after this long.
pub const RESYNC_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeakAt {
    Log,
    Journal,
    Publish,
    Mirror,
    Argv,
    Run,
    Answer,
    Inbox,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SecretPlant {
    pub leak: Option<LeakAt>,
    pub answer_twice: bool,
    pub release_off_primary: bool,
    pub keep_grants: bool,
    pub misroute: bool,
    pub worker_accepts_anything: bool,
    pub refresh: bool,
    pub skip_approval: bool,
    pub child_echo: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub released: u64,
    pub stored: u64,
    pub answered: u64,
    pub revoked: u64,
    pub consumed: u64,
    pub resynced: u64,
    pub stale_releases: u64,
    pub split_accepts: u64,
}

#[derive(Clone, Debug)]
pub enum Path<'a> {
    Vault,
    Log,
    Journal,
    Publish,
    Mirror,
    Inbox,
    Argv,
    Answer,
    Audit,
    Wire { op: &'a str, to: &'a NodeId },
    Run { op: &'a str },
    Store,
    ChildEnv,
}

impl Path<'_> {
    fn name(&self) -> String {
        match self {
            Path::Vault => "vault".to_string(),
            Path::Log => "log".to_string(),
            Path::Journal => "journal".to_string(),
            Path::Publish => "publish".to_string(),
            Path::Mirror => "mirror".to_string(),
            Path::Inbox => "inbox".to_string(),
            Path::Argv => "argv".to_string(),
            Path::Answer => "answer".to_string(),
            Path::Audit => "audit".to_string(),
            Path::Wire { op, .. } => format!("wire:{op}"),
            Path::Run { op } => format!("run:{op}"),
            Path::Store => "store".to_string(),
            Path::ChildEnv => "child-env".to_string(),
        }
    }
}

/// What a grant holds: where it was meant to go and which release made it.
#[derive(Clone, Debug)]
pub(super) struct Stored {
    pub expires_at: u64,
    pub stored_at: u64,
    pub value: String,
    pub intended: Option<NodeId>,
    pub release: Option<usize>,
}

/// One release by a vault node, fixed when it happens: nothing here is
/// looked up by (job, name) afterwards.
#[derive(Clone, Debug)]
pub struct Release {
    pub node: NodeId,
    pub term: Term,
    pub newest: Term,
    pub home: NodeId,
    pub vault: NodeId,
    pub job: String,
    pub target: NodeId,
    /// Each asked name and the approval item that allowed it.
    pub asked: Vec<(String, Option<String>)>,
    pub at_ms: u64,
}

/// Where one marker value may go.
#[derive(Clone, Debug)]
pub(super) struct Mark {
    pub target: NodeId,
    pub release: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct StaleAccept {
    pub node: NodeId,
    pub release: Term,
}

#[derive(Clone, Debug, Default)]
pub struct SimSecrets {
    pub(super) plant: SecretPlant,
    pub(super) vault_node: Option<NodeId>,
    pub(super) vault: BTreeMap<String, (String, bool)>,
    pub(super) stores: BTreeMap<NodeId, BTreeMap<(String, String), Stored>>,
    pub(super) started_on: BTreeSet<(NodeId, String)>,
    pub(super) local_ended: BTreeMap<(NodeId, String), u64>,
    pub(super) live_seen: BTreeMap<NodeId, BTreeSet<String>>,
    pub(super) due: BTreeMap<NodeId, u64>,
    pub(super) revoke_seq: u64,
    pub(super) revokes: Vec<(u64, NodeId, String, Option<Vec<String>>)>,
    pub(super) releases: Vec<Release>,
    pub(super) marks: BTreeMap<String, Mark>,
    pub(super) next_token: u64,
    pub(super) stale_accepts: Vec<StaleAccept>,
    pub(super) login_runs: Vec<(NodeId, Vec<String>)>,
    pub(super) answered: BTreeMap<String, u32>,
    pub(super) leak: Option<(NodeId, String)>,
    pub(super) books: BTreeMap<NodeId, Book>,
    pub(super) mirrors: BTreeMap<NodeId, BTreeMap<String, bool>>,
    pub(super) synced: BTreeMap<(NodeId, NodeId), u64>,
    pub(super) pending: BTreeMap<u64, super::secrets_broker::Pending>,
    pub(super) waiting: BTreeMap<(NodeId, u64), u64>,
    pub(super) consuming: BTreeMap<(NodeId, u64), String>,
    pub(super) approvals: BTreeMap<String, u64>,
    pub(super) last_approval: Option<String>,
    pub(super) planted: Vec<(NodeId, Effect)>,
    pub(super) next_id: u64,
    pub(super) child_env: u64,
    pub(super) leaked_once: bool,
    pub(super) stats: Stats,
}

/// Every `NL-MARKER-<digits>` in a text.
pub(super) fn tokens_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(MARKER) {
        let tail = &rest[at + MARKER.len()..];
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        out.push(format!("{MARKER}{digits}"));
        rest = &tail[digits.len()..];
    }
    out
}

impl SimSecrets {
    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn child_env_count(&self) -> u64 {
        self.child_env
    }

    pub fn login_runs(&self) -> &[(NodeId, Vec<String>)] {
        &self.login_runs
    }

    pub fn answered(&self, item: &str) -> u32 {
        self.answered.get(item).copied().unwrap_or(0)
    }

    pub fn grants_on(&self, node: &NodeId) -> Vec<(String, String)> {
        self.stores
            .get(node)
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn mirrored(&self, node: &NodeId) -> Vec<String> {
        self.mirrors
            .get(node)
            .map(|m| {
                m.iter()
                    .filter(|(_, open)| **open)
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn revoke_seq(&self) -> u64 {
        self.revoke_seq
    }

    pub fn releases(&self) -> &[Release] {
        &self.releases
    }

    pub(super) fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// A fresh marker value, bound to the one node it may reach.
    pub(super) fn new_mark(&mut self, target: &NodeId, release: Option<usize>) -> String {
        self.next_token += 1;
        let token = format!("{MARKER}{}", 1_000 + self.next_token);
        let mark = Mark {
            target: target.clone(),
            release,
        };
        self.marks.insert(token.clone(), mark);
        token
    }

    pub(super) fn mark_of(&self, text: &str) -> Option<&Mark> {
        tokens_in(text).first().and_then(|t| self.marks.get(t))
    }

    fn bound_to(&self, text: &str, node: &NodeId) -> bool {
        let found = tokens_in(text);
        !found.is_empty()
            && found
                .iter()
                .all(|t| self.marks.get(t).is_some_and(|m| m.target == *node))
    }

    fn allowed(&self, node: &NodeId, path: &Path<'_>, text: &str) -> bool {
        match path {
            Path::Vault => self.vault_node.as_ref() == Some(node),
            Path::Wire {
                op: "grant.deliver",
                to,
            } => self.bound_to(text, to),
            Path::Run {
                op: "grant.deliver",
            }
            | Path::Store
            | Path::ChildEnv => self.bound_to(text, node),
            _ => false,
        }
    }

    pub(super) fn record(&mut self, node: &NodeId, path: Path<'_>, text: &str) {
        if !text.contains(MARKER) || self.allowed(node, &path, text) {
            return;
        }
        if self.leak.is_none() {
            self.leak = Some((node.clone(), path.name()));
        }
    }
}
