use crate::acting::Acting;
use crate::cluster_move::raise_notice;
use crate::cluster_promote::{PromoteParts, Replier};
use crate::inbox_ops::{InboxOps, Responder};
use notchling_engine::cluster::outward::CutStep;
use notchling_engine::cluster::text::{plain, NAME_MAX};
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use std::sync::Arc;

pub const CONTINUE_TEXT: &str = "Continue";
const SUMMARY_MAX: usize = 120;

#[derive(Clone)]
pub struct Items {
    pub(crate) inbox: Arc<InboxOps>,
    pub(crate) acting: Arc<Acting>,
    replier: Arc<dyn Replier>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

pub fn action(id: ActionId, label: &str, primary: bool) -> Action {
    Action {
        id,
        label: label.to_string(),
        primary,
    }
}

pub fn open_item(id: &str, kind: Kind, (title, detail): (String, String), now: i64) -> Item {
    Item {
        id: id.to_string(),
        kind,
        title,
        detail,
        source: Source::default(),
        created_ms: now,
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

struct Continuing {
    inbox: Arc<InboxOps>,
    acting: Arc<Acting>,
    replier: Arc<dyn Replier>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    session: String,
}

impl Responder for Continuing {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        match action {
            ActionId::Done => true,
            ActionId::Continue => self.send(),
            _ => false,
        }
    }
}

impl Continuing {
    fn send(&self) -> bool {
        if !self.acting.is_on() {
            return false;
        }
        let (inbox, replier, clock) =
            (self.inbox.clone(), self.replier.clone(), self.clock.clone());
        let session = self.session.clone();
        std::thread::Builder::new()
            .name("cluster-promote-continue".into())
            .spawn(move || {
                if let Err(why) = replier.reply(&session, CONTINUE_TEXT) {
                    let id = format!("paused-step-failed:{session}");
                    let title = "Couldn't send Continue".to_string();
                    raise_notice(&inbox, clock(), &id, (Kind::ActionFailed, &title, &why));
                }
            })
            .is_ok()
    }
}

impl Items {
    pub fn new(p: &PromoteParts) -> Items {
        Items {
            inbox: p.mover.p.inbox.clone(),
            acting: p.acting.clone(),
            replier: p.replier.clone(),
            clock: p.mover.p.clock.clone(),
        }
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    pub fn paused_step(&self, session: &str, k: usize, old: &str, step: &CutStep) {
        let id = format!("paused-step:{session}:{k}");
        let summary = plain(&step.summary, SUMMARY_MAX);
        let title = format!(
            "{} dropped while {summary} was running. Check, then Continue.",
            plain(old, NAME_MAX)
        );
        let detail = if step.outward {
            "It may have changed something outside this machine, so it was not run again."
        } else {
            "It was not run again."
        };
        let mut item = open_item(
            &id,
            Kind::PausedStep,
            (title, detail.to_string()),
            self.now(),
        );
        item.source.session = Some(session.to_string());
        item.actions = vec![
            action(ActionId::Continue, "Continue", true),
            action(ActionId::Done, "Done", false),
        ];
        let responder = Arc::new(Continuing {
            inbox: self.inbox.clone(),
            acting: self.acting.clone(),
            replier: self.replier.clone(),
            clock: self.clock.clone(),
            session: session.to_string(),
        });
        self.inbox.raise(item, Some(responder));
    }

    pub fn failover(&self, term_n: u64, text: &str) {
        let id = format!("failover:{term_n}");
        raise_notice(&self.inbox, self.now(), &id, (Kind::Failover, text, ""));
    }

    pub fn failed(&self, id: &str, title: &str, detail: &str) {
        raise_notice(
            &self.inbox,
            self.now(),
            id,
            (Kind::ActionFailed, title, detail),
        );
    }
}
