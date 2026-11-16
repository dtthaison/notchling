use crate::cluster_move::raise_notice;
use crate::cluster_promote_items::{action, open_item};
use crate::cluster_returns::Returns;
use crate::inbox_ops::Responder;
use notchling_engine::cluster::text::{plain, NAME_MAX};
use notchling_engine::cluster::Member;
use notchling_engine::inbox::{ActionId, Item, Kind, State};
use std::sync::{Arc, Weak};

const TITLE_MAX: usize = 80;

pub fn back_id(term: u64) -> String {
    format!("primary-back:{term}")
}

pub fn waits_id(session: &str) -> String {
    format!("session-waits:{session}")
}

fn is_waits_of(item: &Item, session: &str) -> bool {
    item.kind == Kind::SessionWaits && item.source.session.as_deref() == Some(session)
}

struct Back {
    returns: Weak<Returns>,
    term: u64,
}

struct Waits {
    returns: Weak<Returns>,
    session: String,
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> bool {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(work)
        .is_ok()
}

impl Responder for Back {
    fn answered(&self, _item: &Item, answered: ActionId) -> bool {
        let Some(returns) = self.returns.upgrade() else {
            return false;
        };
        match answered {
            ActionId::Keep => true,
            ActionId::TakeBack if returns.p.acting.is_on() => {
                let term = self.term;
                spawn("cluster-take-back", move || {
                    if let Err(e) = returns.take_back() {
                        returns.failed(
                            &format!("take-back-failed:{term}"),
                            "Couldn't take back the primary",
                            &e.message,
                        );
                    }
                })
            }
            _ => false,
        }
    }
}

impl Responder for Waits {
    fn answered(&self, _item: &Item, answered: ActionId) -> bool {
        let Some(returns) = self.returns.upgrade() else {
            return false;
        };
        match answered {
            ActionId::WaitFor => true,
            ActionId::ContinueHere if returns.p.acting.is_on() => {
                let session = self.session.clone();
                spawn("cluster-continue-here", move || {
                    if let Err(e) = returns.continue_here(&session) {
                        let title = format!("Couldn't continue {}", returns.title_of(&session));
                        returns.failed(
                            &format!("continue-here-failed:{session}"),
                            &title,
                            &e.message,
                        );
                    }
                })
            }
            _ => false,
        }
    }
}

impl Returns {
    pub fn title_of(&self, session: &str) -> String {
        let named = self
            .p
            .mover
            .agents()
            .into_iter()
            .find(|agent| agent.id == session)
            .map(|agent| plain(&agent.name, TITLE_MAX))
            .filter(|name| !name.is_empty());
        named.unwrap_or_else(|| session.to_string())
    }

    fn now(&self) -> i64 {
        self.p.mover.now()
    }

    pub(crate) fn failed(&self, id: &str, title: &str, detail: &str) {
        let inbox = &self.p.mover.p.inbox;
        raise_notice(inbox, self.now(), id, (Kind::ActionFailed, title, detail));
    }

    pub(crate) fn primary_back(self: &Arc<Self>, term: u64, newer: &Member) {
        let name = plain(&newer.name, NAME_MAX);
        let id = back_id(term);
        if !self.remember(|m| m.raised.insert(id.clone())) {
            return;
        }
        let detail =
            "Take Back moves the primary here. Work this machine did after the last copy is kept.";
        let mut item = open_item(
            &id,
            Kind::PrimaryBack,
            (format!("{name} is primary"), detail.to_string()),
            self.now(),
        );
        item.actions = vec![
            action(ActionId::TakeBack, "Take Back", true),
            action(ActionId::Keep, &format!("Keep {name}"), false),
        ];
        let responder = Arc::new(Back {
            returns: Arc::downgrade(self),
            term,
        });
        self.p.mover.p.inbox.raise(item, Some(responder));
    }

    pub(crate) fn withdraw_back_except(&self, term: Option<u64>) {
        let keep = term.map(back_id);
        let gone: Vec<String> = self.remember(|m| {
            let ids: Vec<String> = m
                .raised
                .iter()
                .filter(|id| id.starts_with("primary-back:") && Some(*id) != keep.as_ref())
                .cloned()
                .collect();
            for id in &ids {
                m.raised.remove(id);
            }
            ids
        });
        for id in gone {
            self.p
                .mover
                .p
                .inbox
                .resolve(&id, State::Withdrawn, None, None);
        }
    }

    pub(crate) fn close_waits(&self, session: &str, state: State, answer: Option<ActionId>) {
        let inbox = &self.p.mover.p.inbox;
        for item in inbox.items() {
            if is_waits_of(&item, session) && item.state == State::Open {
                inbox.resolve(&item.id, state, answer, None);
            }
        }
    }

    pub(crate) fn sync_waiting(self: &Arc<Self>) {
        let list = self.p.waiting.list();
        let inbox = &self.p.mover.p.inbox;
        let items = inbox.items();
        for item in &items {
            let stale = item.kind == Kind::SessionWaits
                && item.state == State::Open
                && !list
                    .iter()
                    .any(|w| item.source.session.as_deref() == Some(w.session.as_str()));
            if stale {
                inbox.resolve(&item.id, State::Withdrawn, None, None);
            }
        }
        for waits in &list {
            let own: Vec<&Item> = items
                .iter()
                .filter(|i| is_waits_of(i, &waits.session))
                .collect();
            if own.iter().any(|i| i.state == State::Open) {
                continue;
            }
            if own.iter().any(|i| i.answer == Some(ActionId::WaitFor)) {
                continue;
            }
            let id = match own.len() {
                0 => waits_id(&waits.session),
                n => format!("{}:{n}", waits_id(&waits.session)),
            };
            self.raise_waits(&id, &waits.session, &waits.name);
        }
    }

    fn raise_waits(self: &Arc<Self>, id: &str, session: &str, name: &str) {
        let title = format!(
            "{} waits for {name}, which has newer work in it",
            self.title_of(session)
        );
        let detail = "Continue From This Copy keeps the other machine's work as a side copy.";
        let mut item = open_item(
            id,
            Kind::SessionWaits,
            (title, detail.to_string()),
            self.now(),
        );
        item.source.session = Some(session.to_string());
        item.actions = vec![
            action(ActionId::WaitFor, &format!("Wait for {name}"), true),
            action(ActionId::ContinueHere, "Continue From This Copy", false),
        ];
        let responder = Arc::new(Waits {
            returns: Arc::downgrade(self),
            session: session.to_string(),
        });
        self.p.mover.p.inbox.raise(item, Some(responder));
    }
}
