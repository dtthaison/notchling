use crate::cluster_move::Mover;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::returns::continue_here;
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::state::Command;
use notchling_engine::cluster::wire::OriginTier;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

pub const CLAIM_WAIT: Duration = Duration::from_secs(30);

pub fn transcript_len(path: &str) -> u64 {
    std::fs::metadata(Path::new(path)).map_or(0, |m| m.len())
}

impl Mover {
    pub(crate) fn claim_here(
        &self,
        id: &str,
        main: &BTreeMap<String, MainSession>,
    ) -> Result<(), String> {
        let Some(state) = persist::load(&self.p.cluster).ok().flatten() else {
            return Ok(());
        };
        let Some(me) = self.me() else {
            return Ok(());
        };
        let Some(previous) = state.claims.get(id).filter(|c| c.node != me) else {
            return Ok(());
        };
        let from_set = StandbyStore::new(&self.p.cluster, &previous.node)
            .current()
            .map(|manifest| manifest.set);
        let base_set = from_set.unwrap_or_else(|| previous.base_set.clone());
        let base_len = main
            .get(id)
            .and_then(|m| m.transcript.as_deref())
            .map_or(0, transcript_len);
        let claim = continue_here(id, &me, &state.term, Some(previous), &base_set, base_len);
        self.p
            .link
            .record(Command::Claim { claim }, OriginTier::Node, CLAIM_WAIT)
            .map_err(|e| format!("its new claim wasn't recorded: {}", e.message))
    }
}
