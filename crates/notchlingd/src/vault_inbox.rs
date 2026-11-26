use super::vault_events::device_now;
use super::*;
use crate::acting_ops::now_ms;
use crate::inbox_ops::{InboxOps, Responder};
use crate::presence::{Presence, PresenceGuard};
use notchling_engine::inbox::{
    Action, ActionId, AlertAction, Item, Kind as ItemKind, Live, Place, Source, State, Waiter,
};
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
    fn give(
        &self,
        action: ActionId,
        by: Answerer,
        device: Option<String>,
        note: Option<&str>,
    ) -> bool {
        let value = match action {
            ActionId::AllowOnce => Decision::Once,
            ActionId::AllowAlways => Decision::Always,
            ActionId::Deny => Decision::Deny,
            _ => return false,
        };
        self.approvals.answer(
            &self.ticket,
            Answered {
                value,
                by,
                device,
                note: note.map(str::to_string),
            },
        )
    }
}

impl Responder for ApprovalResponder {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        self.give(action, Answerer::Mac, None, None)
    }

    fn answered_by(&self, _item: &Item, action: ActionId, by: &str) -> bool {
        self.give(action, answerer(by), device_now(), None)
    }

    fn answered_with(&self, _item: &Item, action: ActionId, by: &str, note: Option<&str>) -> bool {
        self.give(action, answerer(by), device_now(), note)
    }
}

pub(super) struct Raised {
    inbox: Option<Arc<InboxOps>>,
    id: String,
    here: Option<(Arc<Presence>, PresenceGuard)>,
}

impl Raised {
    fn new(inbox: &Arc<InboxOps>, id: String, here: (Arc<Presence>, PresenceGuard)) -> Raised {
        Raised {
            inbox: Some(inbox.clone()),
            id,
            here: Some(here),
        }
    }

    fn none() -> Raised {
        Raised {
            inbox: None,
            id: String::new(),
            here: None,
        }
    }

    fn leave(&mut self) {
        if let Some((presence, guard)) = self.here.take() {
            drop(guard);
            presence.forget(&self.id);
        }
    }

    pub(super) fn note(&self, note: &str) {
        if let Some(inbox) = &self.inbox {
            inbox.note_item(&self.id, note);
        }
    }

    pub(super) fn close(mut self, state: State, answer: Option<ActionId>, by: Option<Answerer>) {
        self.leave();
        if let Some(inbox) = self.inbox.take() {
            let by = by.filter(|_| state == State::Answered).map(Answerer::name);
            inbox.resolve(&self.id, state, answer, by);
        }
    }
}

impl Drop for Raised {
    fn drop(&mut self) {
        self.leave();
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
        switch: None,
        ..Default::default()
    }
}

fn approval_item_id(ticket: &str) -> String {
    format!("vault-approval:{ticket}")
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
        (command, folder): (&str, &str),
        by: Option<&ReleaseFor>,
        session: Option<String>,
    ) -> Raised {
        let Some(inbox) = &self.inbox else {
            return Raised::none();
        };
        let command: String = command.chars().take(COMMAND_CUT).collect();
        let mut raised = item(
            approval_item_id(ticket),
            ItemKind::SecretApproval,
            match by {
                Some(grant) => format!(
                    "{command} on {} (job {}) wants {}",
                    grant.node_name,
                    grant.job,
                    names.join(", ")
                ),
                None => format!("Allow {} for {command}", names.join(", ")),
            },
            folder.to_string(),
        );
        let lasts = self.timing.approval_wait + self.timing.slice;
        raised.expires_ms = Some(raised.created_ms.saturating_add(wait_ms(lasts)));
        raised.actions = vec![
            action(ActionId::AllowOnce, "Allow Once", true),
            action(ActionId::AllowAlways, "Always Allow", false),
            action(ActionId::Deny, "Deny", false),
        ];
        raised.alert_actions = vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change];
        raised.place = Some(Place {
            session,
            machine_name: by.map(|grant| grant.node_name.clone()),
            ..Place::default()
        })
        .filter(|place| place.session.is_some() || place.machine_name.is_some());
        raised.open = Some(json!({"vault_approval": ticket}));
        let responder = ApprovalResponder {
            approvals: self.approvals.clone(),
            ticket: ticket.to_string(),
        };
        let created = raised.created_ms;
        raised.blocking = Some(true);
        raised.waiting_since = Some(created);
        raised.waiter = Some(Waiter::Vault);
        let id = inbox.raise(raised, Some(Arc::new(responder)));
        let guard = self.presence.enter(&id, created);
        self.sweep_live(inbox);
        Raised::new(inbox, id, (self.presence.clone(), guard))
    }

    fn sweep_live(&self, inbox: &Arc<InboxOps>) {
        for (id, here) in self.presence.sweep(now_ms()) {
            let live = here.then(|| Live {
                waiting_since: self.presence.since(&id).unwrap_or_else(now_ms),
                waiter: Waiter::Vault,
            });
            inbox.set_live(&id, live);
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
