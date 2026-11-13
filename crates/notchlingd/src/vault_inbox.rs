use super::*;
use crate::acting_ops::now_ms;
use crate::inbox_ops::Responder;
use notchling_engine::inbox::{Action, ActionId, Item, Kind as ItemKind, Source, State};
use std::time::Instant;

const COMMAND_CUT: usize = 120;

struct ApprovalResponder {
    approvals: Arc<Waiters<Decision>>,
    ticket: String,
}

impl Responder for ApprovalResponder {
    fn answered(&self, _item: &Item, action: ActionId) {
        let decision = match action {
            ActionId::AllowOnce => Decision::Once,
            ActionId::AllowAlways => Decision::Always,
            ActionId::Deny => Decision::Deny,
            _ => return,
        };
        self.approvals.answer(&self.ticket, decision);
    }
}

struct RequestResponder {
    requests: Arc<Waiters<Option<VaultEntry>>>,
    ticket: String,
}

impl Responder for RequestResponder {
    fn answered(&self, _item: &Item, action: ActionId) {
        if action == ActionId::Cancel {
            self.requests.answer(&self.ticket, None);
        }
    }
}

fn action(id: ActionId, label: &str, primary: bool) -> Action {
    Action {
        id,
        label: label.to_string(),
        primary,
    }
}

fn item(id: String, kind: ItemKind, title: String, detail: String) -> Item {
    Item {
        id,
        kind,
        title,
        detail,
        source: Source::default(),
        created_ms: now_ms(),
        expires_ms: None,
        actions: Vec::new(),
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
    }
}

pub(super) fn approval_item_id(ticket: &str) -> String {
    format!("vault-approval:{ticket}")
}

pub(super) fn request_item_id(ticket: &str) -> String {
    format!("vault-request:{ticket}")
}

pub(super) fn action_of(decision: Decision) -> ActionId {
    match decision {
        Decision::Once => ActionId::AllowOnce,
        Decision::Always => ActionId::AllowAlways,
        Decision::Deny => ActionId::Deny,
    }
}

impl VaultOps {
    pub(super) fn raise_approval(&self, ticket: &str, names: &[&str], command: &str, folder: &str) {
        let Some(inbox) = &self.inbox else {
            return;
        };
        let command: String = command.chars().take(COMMAND_CUT).collect();
        let mut raised = item(
            approval_item_id(ticket),
            ItemKind::SecretApproval,
            format!("Allow {} for {command}", names.join(", ")),
            folder.to_string(),
        );
        raised.expires_ms = Some(
            raised
                .created_ms
                .saturating_add(wait_ms(self.timing.approval_wait)),
        );
        raised.actions = vec![
            action(ActionId::AllowOnce, "Allow Once", true),
            action(ActionId::AllowAlways, "Always Allow", false),
            action(ActionId::Deny, "Deny", false),
        ];
        raised.open = Some(json!({"vault_approval": ticket}));
        let responder = ApprovalResponder {
            approvals: self.approvals.clone(),
            ticket: ticket.to_string(),
        };
        inbox.raise(raised, Some(Arc::new(responder)));
    }

    pub(super) fn raise_request(&self, ticket: &str, request: &VaultRequest) {
        let Some(inbox) = &self.inbox else {
            return;
        };
        let detail = request
            .reason
            .clone()
            .or_else(|| request.note.clone())
            .unwrap_or_default();
        let mut raised = item(
            request_item_id(ticket),
            ItemKind::SecretRequest,
            format!("Add secret {}", request.name),
            detail,
        );
        raised.actions = vec![
            action(ActionId::Open, "Open Form", true),
            action(ActionId::Cancel, "Cancel", false),
        ];
        raised.open = Some(json!({"vault_request": ticket}));
        let responder = RequestResponder {
            requests: self.requests.clone(),
            ticket: ticket.to_string(),
        };
        inbox.raise(raised, Some(Arc::new(responder)));
    }

    pub(super) fn close_item(&self, id: &str, state: State, answer: Option<ActionId>) {
        if let Some(inbox) = &self.inbox {
            inbox.resolve(id, state, answer);
        }
    }

    pub(super) fn ended_without_answer(
        &self,
        started: Instant,
        within: Duration,
        still: &dyn Fn() -> bool,
    ) -> State {
        if started.elapsed() >= within && still() {
            State::Expired
        } else {
            State::Withdrawn
        }
    }
}

fn wait_ms(wait: Duration) -> i64 {
    i64::try_from(wait.as_millis()).unwrap_or(i64::MAX)
}
