use crate::cluster_promote::Promoter;
use notchling_engine::cluster::failover::Grantor;
use notchling_engine::cluster::members::MemberState;
use notchling_engine::cluster::promote::{Facts, MovingSession, SetFacts};
use notchling_engine::cluster::snapshot::{Manifest, Movable, PartKind};
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::state::ClusterState;
use notchling_engine::cluster::{Member, NodeId, Tool};
use serde_json::Value;

pub fn has_moving(set: &SetFacts) -> bool {
    set.sessions.iter().any(moves)
}

pub fn moves(session: &MovingSession) -> bool {
    session.movable == Movable::Yes && matches!(session.tool, Tool::Claude | Tool::Codex)
}

pub fn moving_ids(facts: &Facts) -> Vec<String> {
    facts.set.as_ref().map_or_else(Vec::new, |set| {
        set.sessions
            .iter()
            .filter(|s| moves(s))
            .map(|s| s.id.clone())
            .collect()
    })
}

pub fn base_len(manifest: &Manifest, transcripts: &[std::path::PathBuf]) -> u64 {
    transcripts
        .iter()
        .filter_map(|path| manifest.parts.iter().find(|part| part.path == *path))
        .map(|part| match part.kind {
            PartKind::Tail { from, .. } => from.saturating_add(part.len),
            _ => part.len,
        })
        .sum()
}

pub fn shared_members(state: &ClusterState) -> Vec<Member> {
    state
        .members
        .values()
        .filter(|m| m.state != MemberState::Pending)
        .cloned()
        .collect()
}

fn set_facts(state: &ClusterState, manifest: &Manifest) -> SetFacts {
    let sessions = manifest
        .sessions
        .iter()
        .map(|entry| MovingSession {
            id: entry.id.clone(),
            tool: entry.tool,
            movable: entry.movable.clone(),
            flags: state
                .main
                .get(&entry.id)
                .map(|m| m.respawn.clone())
                .unwrap_or_default(),
            previous: state.claims.get(&entry.id).cloned(),
            base_len: base_len(manifest, &entry.transcripts),
        })
        .collect();
    SetFacts {
        id: manifest.set.clone(),
        versions: state
            .members
            .get(&state.home)
            .and_then(|m| m.versions.clone())
            .unwrap_or_else(crate::cluster_versions::own),
        sessions,
    }
}

fn reaches(status: &Value, id: &NodeId) -> bool {
    status["members"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|m| m["id"].as_str() == Some(id.as_str()) && m["online"].as_bool() == Some(true))
}

impl Promoter {
    pub(crate) fn facts(&self, state: ClusterState, me: &NodeId) -> Facts {
        let status = self.p.cluster.status();
        let ready = (self.p.mover.p.ready)(&status, me);
        let peers: Vec<NodeId> = state
            .active()
            .filter(|m| m.can_be_master && m.id != *me && m.id != state.home)
            .filter(|m| reaches(&status, &m.id))
            .map(|m| m.id.clone())
            .collect();
        let mine = state
            .members
            .get(me)
            .and_then(|m| m.versions.clone())
            .unwrap_or_else(crate::cluster_versions::own);
        let set = StandbyStore::new(&self.p.mover.p.cluster, &state.home)
            .current()
            .map(|manifest| set_facts(&state, &manifest));
        Facts {
            grantor: Grantor::new((self.p.boot.now)()),
            state,
            mine,
            ready,
            peers,
            set,
        }
    }
}
