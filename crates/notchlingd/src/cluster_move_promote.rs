use crate::cluster_move::{failed, Mover};
use crate::cluster_node::{api_error, refused};
use crate::snap_restore_fs::staged_db;
use notchling_engine::cluster::core::{eligible, Takeover};
use notchling_engine::cluster::NodeId;
use notchling_local_api::ApiError;
use serde_json::Value;
use std::collections::BTreeSet;

impl Mover {
    pub fn take_over(&self, takeover: Takeover, old_home: &NodeId) -> Result<Value, ApiError> {
        let me = self
            .me()
            .ok_or_else(|| failed("this machine is in no cluster"))?;
        let name = self.name_of(&me);
        if takeover.term.node != me {
            return Err(refused("that term isn't this machine's to take"));
        }
        if !eligible(&takeover, &me) {
            return Err(refused(format!(
                "{name} can't be primary: it isn't an active Can Be Master member"
            )));
        }
        if takeover.sessions.is_empty() {
            return self.p.link.take_primary(takeover).map_err(api_error);
        }
        let sessions: BTreeSet<String> = takeover.sessions.iter().cloned().collect();
        self.p
            .restore
            .check(old_home, &name, takeover.set.as_deref(), &sessions)
            .map_err(refused)?;
        let stage = staged_db(&self.p.cluster);
        let _ = std::fs::remove_file(&stage);
        let answer = self.stage_and_take(takeover, old_home, &name, &stage);
        if answer.is_err() {
            let _ = std::fs::remove_file(&stage);
        }
        answer
    }
}
