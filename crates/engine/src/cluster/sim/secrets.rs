use std::collections::{BTreeMap, BTreeSet};

use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use crate::inbox::Book;

pub const MARKER: &str = "NL-MARKER-";
pub const JANITOR_MS: u64 = 30_000;
pub const GRANT_LIFE_MS: u64 = 12 * 3_600_000;
pub const KNOWN_WAIT_MS: u64 = 3_000;
pub const STEADY_MS: u64 = JANITOR_MS + 5_000;
pub const RECOVERY_MS: u64 = JANITOR_MS + 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeakAt {
    Log,
    Journal,
    Publish,
    Mirror,
    Argv,
    Run,
    Answer,
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
    Argv,
    Answer,
    Audit,
    Wire {
        op: &'a str,
        to: &'a NodeId,
        job: Option<&'a str>,
    },
    Run {
        op: &'a str,
        job: Option<&'a str>,
    },
    Store {
        job: &'a str,
    },
    ChildEnv {
        job: &'a str,
    },
}

impl Path<'_> {
    fn name(&self) -> String {
        match self {
            Path::Vault => "vault".to_string(),
            Path::Log => "log".to_string(),
            Path::Journal => "journal".to_string(),
            Path::Publish => "publish".to_string(),
            Path::Mirror => "mirror".to_string(),
            Path::Argv => "argv".to_string(),
            Path::Answer => "answer".to_string(),
            Path::Audit => "audit".to_string(),
            Path::Wire { op, .. } => format!("wire:{op}"),
            Path::Run { op, .. } => format!("run:{op}"),
            Path::Store { .. } => "store".to_string(),
            Path::ChildEnv { .. } => "child-env".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Stored {
    pub expires_at: u64,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct ReleaseRec {
    pub node: NodeId,
    pub term: Term,
    pub home: NodeId,
    pub vault: NodeId,
    pub job: String,
    pub asked: Vec<String>,
    pub at_ms: u64,
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
    pub(super) truth: BTreeMap<String, BTreeSet<NodeId>>,
    pub(super) stores: BTreeMap<NodeId, BTreeMap<(String, String), Stored>>,
    pub(super) started_on: BTreeSet<(NodeId, String)>,
    pub(super) local_ended: BTreeMap<(NodeId, String), u64>,
    pub(super) ended: BTreeMap<(NodeId, String), u64>,
    pub(super) reach: BTreeMap<NodeId, Option<u64>>,
    pub(super) last_tick: BTreeMap<NodeId, u64>,
    pub(super) alive_since: BTreeMap<NodeId, u64>,
    pub(super) due: BTreeMap<NodeId, u64>,
    pub(super) revoke_seq: u64,
    pub(super) revokes: Vec<(u64, NodeId, String, Option<Vec<String>>)>,
    pub(super) released_terms: BTreeMap<(String, String), Term>,
    pub(super) releases: Vec<ReleaseRec>,
    pub(super) stale_accepts: Vec<StaleAccept>,
    pub(super) login_runs: Vec<(NodeId, Vec<String>)>,
    pub(super) answered: BTreeMap<String, u32>,
    pub(super) leak: Option<(NodeId, String)>,
    pub(super) books: BTreeMap<NodeId, Book>,
    pub(super) mirrors: BTreeMap<NodeId, BTreeMap<String, bool>>,
    pub(super) resync: BTreeSet<(NodeId, NodeId)>,
    pub(super) pending: BTreeMap<u64, super::secrets_broker::Pending>,
    pub(super) waiting: BTreeMap<(NodeId, u64), u64>,
    pub(super) approvals: BTreeMap<String, u64>,
    pub(super) next_id: u64,
    pub(super) child_env: u64,
    pub(super) leaked_once: bool,
    pub(super) stats: Stats,
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

    pub(super) fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn allowed(&self, node: &NodeId, path: &Path<'_>) -> bool {
        let on_job_node =
            |job: &str, at: &NodeId| self.truth.get(job).is_some_and(|s| s.contains(at));
        match path {
            Path::Vault => self.vault_node.as_ref() == Some(node),
            Path::Wire {
                op: "grant.deliver",
                to,
                job: Some(job),
            } => on_job_node(job, to),
            Path::Run {
                op: "grant.deliver",
                job: Some(job),
            } => on_job_node(job, node),
            Path::Store { job } | Path::ChildEnv { job } => on_job_node(job, node),
            _ => false,
        }
    }

    pub(super) fn record(&mut self, node: &NodeId, path: Path<'_>, text: &str) {
        if !text.contains(MARKER) || self.allowed(node, &path) {
            return;
        }
        if self.leak.is_none() {
            self.leak = Some((node.clone(), path.name()));
        }
    }
}
