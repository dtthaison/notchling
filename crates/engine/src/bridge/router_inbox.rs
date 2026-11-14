use super::connection::Connection;
use super::inbox::{action_from, answer_json, name_of, wire_item, InboxOutcome};
use super::router::Router;
use super::router_ops::require;
use super::wire::{self, BridgeFailure};
use crate::inbox::{ActionId, State};
use serde_json::{json, Map, Value};
use std::sync::Arc;

type Refusal = (&'static str, BridgeFailure);

impl Router {
    pub(crate) fn inbox_list(&self) -> Value {
        let items = self.backend.inbox_items();
        let unread = items
            .iter()
            .filter(|item| item.state == State::Open && !item.seen)
            .count();
        let wire: Vec<_> = items.iter().map(wire_item).collect();
        json!({"items": wire::object(&wire), "unread": unread})
    }

    pub(crate) fn inbox_answer(
        &self,
        body: &Map<String, Value>,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let decided = self.inbox_decide(body);
        let op = match &decided {
            Ok((action, outcome)) if outcome.answered => {
                format!("inbox.answer:{}", name_of(action))
            }
            Ok(_) => "inbox.answer:refused:not_taken".to_string(),
            Err((reason, _)) => format!("inbox.answer:refused:{reason}"),
        };
        self.audit(connection, &op, None, None);
        decided
            .map(|(_, outcome)| answer_json(&outcome))
            .map_err(|(_, failure)| failure)
    }

    fn inbox_decide(&self, body: &Map<String, Value>) -> Result<(ActionId, InboxOutcome), Refusal> {
        if !self.backend.acting_on() {
            return Err((
                "acting_off",
                BridgeFailure::invalid(super::live::ACTING_OFF),
            ));
        }
        let invalid = |failure| ("invalid", failure);
        let item_id = require(body, "itemId").map_err(invalid)?;
        let action = action_from(require(body, "action").map_err(invalid)?)
            .ok_or_else(|| invalid(BridgeFailure::invalid("The action is not valid.")))?;
        if action == ActionId::Open {
            return Err(("open", BridgeFailure::invalid("Open the form on the Mac.")));
        }
        let outcome = self
            .backend
            .inbox_answer(item_id, action)
            .map_err(|failure| (wire::code_name(failure.code), failure))?;
        Ok((action, outcome))
    }
}
