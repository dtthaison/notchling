use super::vault_events::device_now;
use super::*;
use crate::acting_ops::now_ms;
use crate::inbox_ops::{InboxOps, Responder};
use notchling_engine::inbox::{Action, ActionId, Item, Kind as ItemKind, Source, State};
use notchling_events::kinds::choices::Answerer;
use std::time::Instant;

const COMMAND_CUT: usize = 120;

fn answerer(by: &str) -> Answerer {
    Answerer::from_name(by).unwrap_or(Answerer::Mac)
}

struct ApprovalResponder {
    approvals: Arc<Waiters<Answered<Decision>>>,
    ticket: String,
}

impl ApprovalResponder {
    fn give(&self, action: ActionId, by: Answerer, device: Option<String>) -> bool {
        let value = match action {
            ActionId::AllowOnce => Decision::Once,
            ActionId::AllowAlways => Decision::Always,
            ActionId::Deny => Decision::Deny,
            _ => return false,
        };
        self.approvals
            .answer(&self.ticket, Answered { value, by, device })
    }
}

impl Responder for ApprovalResponder {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        self.give(action, Answerer::Mac, None)
    }

    fn answered_by(&self, _item: &Item, action: ActionId, by: &str) -> bool {
        self.give(action, answerer(by), device_now())
    }
}

struct RequestResponder {
    requests: Arc<Waiters<Answered<Option<VaultEntry>>>>,
    ticket: String,
}

impl RequestResponder {
    fn give(&self, action: ActionId, by: Answerer, device: Option<String>) -> bool {
        action == ActionId::Cancel
            && self.requests.answer(
                &self.ticket,
                Answered {
                    value: None,
                    by,
                    device,
                },
            )
    }
}

impl Responder for RequestResponder {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        self.give(action, Answerer::Mac, None)
    }

    fn answered_by(&self, _item: &Item, action: ActionId, by: &str) -> bool {
        self.give(action, answerer(by), device_now())
    }
}

pub(super) struct Raised {
    inbox: Option<Arc<InboxOps>>,
    id: String,
}

impl Raised {
    fn new(inbox: &Arc<InboxOps>, id: String) -> Raised {
        Raised {
            inbox: Some(inbox.clone()),
            id,
        }
    }

    fn none() -> Raised {
        Raised {
            inbox: None,
            id: String::new(),
        }
    }

    pub(super) fn close(mut self, state: State, answer: Option<ActionId>, by: Option<Answerer>) {
        if let Some(inbox) = self.inbox.take() {
            let by = by.filter(|_| state == State::Answered).map(Answerer::name);
            inbox.resolve(&self.id, state, answer, by);
        }
    }
}

impl Drop for Raised {
    fn drop(&mut self) {
        if let Some(inbox) = self.inbox.take() {
            inbox.resolve(&self.id, State::Withdrawn, None, None);
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

fn approval_item_id(ticket: &str) -> String {
    format!("vault-approval:{ticket}")
}

fn request_item_id(ticket: &str) -> String {
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
    pub(super) fn raise_approval(
        &self,
        ticket: &str,
        names: &[&str],
        command: &str,
        folder: &str,
    ) -> Raised {
        let Some(inbox) = &self.inbox else {
            return Raised::none();
        };
        let command: String = command.chars().take(COMMAND_CUT).collect();
        let mut raised = item(
            approval_item_id(ticket),
            ItemKind::SecretApproval,
            format!("Allow {} for {command}", names.join(", ")),
            folder.to_string(),
        );
        let lasts = self.timing.approval_wait + self.timing.slice;
        raised.expires_ms = Some(raised.created_ms.saturating_add(wait_ms(lasts)));
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
        Raised::new(inbox, inbox.raise(raised, Some(Arc::new(responder))))
    }

    pub(super) fn raise_request(&self, ticket: &str, request: &VaultRequest) -> Raised {
        let Some(inbox) = &self.inbox else {
            return Raised::none();
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
        Raised::new(inbox, inbox.raise(raised, Some(Arc::new(responder))))
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
