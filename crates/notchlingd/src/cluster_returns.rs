use crate::acting::Acting;
use crate::cluster_move::{failed, lock, Mover};
use crate::cluster_returns_aside::{aside, Aside};
use crate::cluster_returns_claim::{transcript_len, CLAIM_WAIT};
use crate::cluster_returns_waiting::Waiting;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::returns::{continue_here, take_back, TakeBackStep};
use notchling_engine::cluster::snapshot::repo_state::RepoFacts;
use notchling_engine::cluster::state::{ClusterState, Command};
use notchling_engine::cluster::wire::OriginTier;
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{ActionId, State};
use notchling_local_api::{ops, ApiError};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const TAKE_BACK_WAIT: Duration = Duration::from_secs(60);
pub const HIDE_WAIT: Duration = Duration::from_secs(30);
pub const NO_BASE_SET: &str = "local";

pub trait Repo: Send + Sync {
    fn changed(&self, cwd: &str, was: Option<&RepoFacts>) -> bool;
}

pub struct ReturnsParts {
    pub mover: Arc<Mover>,
    pub repo: Arc<dyn Repo>,
    pub acting: Arc<Acting>,
    pub data: PathBuf,
    pub waiting: Arc<Waiting>,
}

#[derive(Default)]
pub(crate) struct Memory {
    pub marker_term: Option<u64>,
    pub raised: BTreeSet<String>,
    pub copied: BTreeSet<(String, u64)>,
    pub touched: BTreeSet<(String, u64)>,
    pub tried: BTreeSet<(String, u64)>,
}

pub struct Returns {
    pub(crate) p: ReturnsParts,
    pub(crate) mem: Mutex<Memory>,
}

impl Returns {
    pub fn new(parts: ReturnsParts) -> Arc<Returns> {
        let returns = Arc::new(Returns {
            p: parts,
            mem: Mutex::new(Memory::default()),
        });
        let weak = Arc::downgrade(&returns);
        returns.p.mover.gate().on_send(Arc::new(move |session| {
            if let Some(returns) = weak.upgrade() {
                returns.before_send(session);
            }
        }));
        returns
    }

    pub(crate) fn cluster(&self) -> &Path {
        &self.p.mover.p.cluster
    }

    pub(crate) fn load(&self) -> Option<ClusterState> {
        persist::load(self.cluster()).ok().flatten()
    }

    fn here(&self) -> Result<(NodeId, ClusterState), ApiError> {
        let me = self
            .p
            .mover
            .me()
            .ok_or_else(|| failed("This machine is in no cluster"))?;
        let state = self
            .load()
            .ok_or_else(|| failed("This machine is in no cluster"))?;
        Ok((me, state))
    }

    pub fn waiting(&self) -> Value {
        let list: Vec<Value> = self
            .p
            .waiting
            .list()
            .into_iter()
            .map(|w| {
                let title = self.title_of(&w.session);
                json!({"session": w.session, "node": w.node, "name": w.name, "title": title})
            })
            .collect();
        Value::Array(list)
    }

    pub fn add_waiting(&self, session: &str, node: &NodeId) -> Result<(), String> {
        let name = self.p.mover.name_of(node);
        self.p.waiting.add(session, node, &name)
    }

    pub fn take_back(&self) -> Result<Value, ApiError> {
        let (me, state) = self.here()?;
        if state.home == me {
            return Err(failed("This machine is already the primary"));
        }
        let steps =
            take_back(&me, &state.home, &state.side_copies, &self.p.data).map_err(failed)?;
        for step in steps {
            match step {
                TakeBackStep::MoveAside { record, from, to } => {
                    let moved = aside(Path::new(&from), Path::new(&to)).map_err(failed)?;
                    if moved == Aside::Nothing {
                        continue;
                    }
                    let mut copy = record;
                    copy.hidden = false;
                    self.p
                        .mover
                        .p
                        .link
                        .record(Command::SideCopy { copy }, OriginTier::Node, CLAIM_WAIT)
                        .map_err(|e| {
                            failed(format!("Couldn't record where {from} went: {}", e.message))
                        })?;
                }
                TakeBackStep::Handover(request) => {
                    let args = json!({"to": request.to});
                    return self
                        .p
                        .mover
                        .p
                        .link
                        .call_app(&request.via, ops::CLUSTER_MOVE_HOME, args, TAKE_BACK_WAIT)
                        .map_err(|e| failed(e.message));
                }
            }
        }
        Err(failed("Taking back has no handover step"))
    }

    pub fn continue_here(&self, id: &str) -> Result<Value, ApiError> {
        let (me, state) = self.here()?;
        if self.p.waiting.get(id).is_none() {
            return Err(failed("That session isn't waiting"));
        }
        let previous = state.claims.get(id);
        let base_set = previous.map_or(NO_BASE_SET, |c| c.base_set.as_str());
        let base_len = state
            .main
            .get(id)
            .and_then(|m| m.transcript.as_deref())
            .map_or(0, transcript_len);
        let claim = continue_here(id, &me, &state.term, previous, base_set, base_len);
        self.p
            .mover
            .p
            .link
            .record(Command::Claim { claim }, OriginTier::Node, CLAIM_WAIT)
            .map_err(|e| {
                failed(format!(
                    "Couldn't claim {id} on this machine: {}",
                    e.message
                ))
            })?;
        self.p
            .waiting
            .remove(id)
            .map_err(|why| failed(format!("{id} is claimed here, but it still waits: {why}")))?;
        self.close_waits(id, State::Answered, Some(ActionId::ContinueHere));
        Ok(json!({"id": id, "claimed": true}))
    }

    pub fn hide(&self, session: &str, node: &NodeId) -> Result<Value, ApiError> {
        let command = Command::HideSideCopy {
            session: session.to_string(),
            node: node.clone(),
        };
        self.p
            .mover
            .p
            .link
            .record(command, OriginTier::App, HIDE_WAIT)
            .map_err(|e| failed(e.message))?;
        Ok(json!({"hidden": true}))
    }

    pub(crate) fn remember<T>(&self, change: impl FnOnce(&mut Memory) -> T) -> T {
        change(&mut lock(&self.mem))
    }
}
