use crate::consent_ops::ConsentOps;
use crate::inbox_ops::Responder;
use notchling_engine::inbox::{ActionId, Item};
use notchling_events::scope;
use serde_json::json;
use std::sync::Weak;

pub(crate) struct Answers {
    pub(crate) ops: Weak<ConsentOps>,
    pub(crate) id: String,
}

impl Answers {
    fn give(&self, action: ActionId, by: &str, device: Option<String>, note: Option<&str>) -> bool {
        let allow = match action {
            ActionId::Allow | ActionId::AllowOnce => true,
            ActionId::Deny => false,
            _ => return false,
        };
        let Some(ops) = self.ops.upgrade() else {
            return false;
        };
        ops.decide_noted(&self.id, allow, (None, note), by, device)
            .is_ok_and(|v| v["answered"] == json!(true))
    }
}

impl Responder for Answers {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        self.give(action, "mac", None, None)
    }

    fn answered_by(&self, _item: &Item, action: ActionId, by: &str) -> bool {
        let device = scope::current()
            .actor
            .and_then(|a| a.device)
            .map(|d| d.as_str().to_owned());
        self.give(action, by, device, None)
    }

    fn answered_with(&self, _item: &Item, action: ActionId, by: &str, note: Option<&str>) -> bool {
        let device = scope::current()
            .actor
            .and_then(|a| a.device)
            .map(|d| d.as_str().to_owned());
        self.give(action, by, device, note)
    }
}
