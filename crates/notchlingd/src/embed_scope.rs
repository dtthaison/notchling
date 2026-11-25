use crate::embed_ops::EmbedOps;
use crate::reads_ops::verified_org;
use notchling_local_api::{Caller, Tier};
use serde_json::{json, Value};

const COUNTS: [&str; 3] = ["corpora", "backlog", "stored"];

impl EmbedOps {
    pub(crate) fn status_for(&self, caller: &Caller) -> Value {
        if caller.tier == Tier::App {
            return self.status();
        }
        let Some(org) = verified_org(caller) else {
            return json!({"reason": "no_session"});
        };
        let mut reply = self.status();
        if let Some(map) = reply.as_object_mut() {
            for key in COUNTS {
                map.remove(key);
            }
            map.insert("organization".into(), json!(org));
        }
        reply
    }
}
