use crate::cluster_fence::turn;
use crate::cluster_move::Mover;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::returns::continue_here;
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::state::Command;
use notchling_engine::cluster::wire::OriginTier;
use notchling_engine::cluster::NodeId;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

pub const CLAIM_WAIT: Duration = Duration::from_secs(30);
pub const LOCAL_WAIT: Duration = Duration::from_secs(5);
const LOCAL_POLL: Duration = Duration::from_millis(20);

pub fn transcript_len(path: &str) -> u64 {
    std::fs::metadata(Path::new(path)).map_or(0, |m| m.len())
}

impl Mover {
    pub(crate) fn turn(&self) -> std::sync::MutexGuard<'static, ()> {
        turn(&self.p.cluster)
    }

    pub(crate) fn wait_local(&self, id: &str, me: &NodeId, within: Duration) -> Result<(), String> {
        let started = std::time::Instant::now();
        loop {
            let held = persist::load(&self.p.cluster)
                .ok()
                .flatten()
                .and_then(|state| state.claims.get(id).map(|c| c.node.clone()));
            if held.as_ref() == Some(me) {
                return Ok(());
            }
            if started.elapsed() >= within {
                return Err("its new claim hasn't reached this machine yet".to_string());
            }
            std::thread::sleep(LOCAL_POLL);
        }
    }

    pub(crate) fn claim_here(
        &self,
        id: &str,
        main: &BTreeMap<String, MainSession>,
    ) -> Result<(), String> {
        let loaded = persist::load(&self.p.cluster)
            .map_err(|why| format!("this machine's cluster record can't be read: {why}"))?;
        let Some(state) = loaded else {
            return Ok(());
        };
        let me = self
            .me()
            .ok_or_else(|| "this machine's cluster node isn't up yet".to_string())?;
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
            .map_err(|e| format!("its new claim wasn't recorded: {}", e.message))?;
        self.wait_local(id, &me, LOCAL_WAIT)
    }
}
