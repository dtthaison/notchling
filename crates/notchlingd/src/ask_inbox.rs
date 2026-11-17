use crate::ask_ops::{AskOps, Mark};
use crate::ask_tick::Msg;
use crate::inbox_ops::{InboxOps, Responder};
use notchling_engine::ask::{
    action_for, answer_text, choice_of, item, may_answer, AskState, By, Row, Who,
};
use notchling_engine::inbox::{ActionId, Item, State};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

pub(crate) struct Link {
    me: Weak<AskOps>,
    inbox: Arc<InboxOps>,
}

pub(crate) struct AskResponder {
    ops: Weak<AskOps>,
    id: String,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Responder for AskResponder {
    fn answered(&self, item: &Item, action: ActionId) -> bool {
        self.answered_by(item, action, "mac")
    }

    fn answered_by(&self, _item: &Item, action: ActionId, by: &str) -> bool {
        let Some(ops) = self.ops.upgrade() else {
            return false;
        };
        if !ops.acting.is_on() {
            return false;
        }
        let by = if by == "phone" { By::Phone } else { By::Mac };
        match action {
            ActionId::Cancel => ops.queue_dismiss(&self.id),
            other => choice_of(other).is_some_and(|n| ops.queue_answer(&self.id, n, by)),
        }
    }
}

impl AskOps {
    pub fn attach(self: &Arc<Self>, inbox: Arc<InboxOps>) {
        let _ = self.link.set(Link {
            me: Arc::downgrade(self),
            inbox,
        });
    }

    pub(crate) fn queue_answer(&self, id: &str, choice: u8, by: By) -> bool {
        let Ok(Some(row)) = self.store.get(id) else {
            return false;
        };
        if may_answer(&row, &Who::Person).is_err() {
            return false;
        }
        let Ok((answer, choice)) = answer_text(&row, None, Some(choice)) else {
            return false;
        };
        let mark = Mark {
            state: AskState::Answered,
            by: Some(by),
        };
        if self.take(id, mark).is_some() {
            return false;
        }
        let msg = Msg::Answer {
            id: id.to_string(),
            answer,
            choice,
            by,
        };
        if self.tx.send(msg).is_err() {
            self.drop_mark(id);
            return false;
        }
        true
    }

    pub(crate) fn queue_dismiss(&self, id: &str) -> bool {
        let Ok(Some(row)) = self.store.get(id) else {
            return false;
        };
        if may_answer(&row, &Who::Person).is_err() {
            return false;
        }
        let mark = Mark {
            state: AskState::Dismissed,
            by: None,
        };
        if self.take(id, mark).is_some() {
            return false;
        }
        if self.tx.send(Msg::Dismiss(id.to_string())).is_err() {
            self.drop_mark(id);
            return false;
        }
        true
    }

    pub(crate) fn apply(&self, msg: Msg) {
        let (id, failed) = match msg {
            Msg::Answer {
                id,
                answer,
                choice,
                by,
            } => {
                let failed = self.finish_answer(&id, &answer, choice, by).is_err();
                (id, failed)
            }
            Msg::Dismiss(id) => {
                let failed = self.finish_dismiss(&id).is_err();
                (id, failed)
            }
            Msg::Stop => return,
        };
        if failed {
            lock(&self.raised).remove(&id);
            self.settled(&id);
        }
    }

    pub fn drain(&self) {
        let mut queued = Vec::new();
        if let Some(rx) = lock(&self.rx).as_ref() {
            while let Ok(msg) = rx.try_recv() {
                queued.push(msg);
            }
        }
        for msg in queued {
            self.apply(msg);
        }
    }

    fn raise_item(&self, link: &Link, row: &Row, now_ms: i64) -> String {
        let responder = Arc::new(AskResponder {
            ops: link.me.clone(),
            id: row.id.clone(),
        });
        let inbox_id = link.inbox.raise(item(row, None, now_ms), Some(responder));
        let _ = self.store.set_inbox(&row.id, Some(&inbox_id));
        inbox_id
    }

    pub(crate) fn settled(&self, id: &str) {
        let Some(link) = self.link.get() else {
            return;
        };
        if !lock(&self.raised).insert(id.to_string()) {
            return;
        }
        let Ok(Some(row)) = self.store.get(id) else {
            return;
        };
        if row.state != AskState::Open {
            return;
        }
        self.raise_item(link, &row, self.now());
        if let Ok(Some(after)) = self.store.get(id) {
            if after.state != AskState::Open {
                self.resolve_item(&after);
            }
        }
    }

    pub(crate) fn raise_again(&self, row: &Row, now_ms: i64) {
        let Some(link) = self.link.get() else {
            return;
        };
        let fresh = if row.state == AskState::Open {
            lock(&self.raised).insert(row.id.clone());
            Some(self.raise_item(link, row, now_ms))
        } else {
            let _ = self.store.set_inbox(&row.id, None);
            None
        };
        if let Some(old) = row
            .inbox_id
            .as_deref()
            .filter(|old| fresh.as_deref() != Some(old))
        {
            link.inbox.remove(old);
        }
    }

    pub(crate) fn resolve_item(&self, row: &Row) {
        let Some(link) = self.link.get() else {
            return;
        };
        let Some(inbox_id) = row.inbox_id.as_deref() else {
            return;
        };
        let (state, answer, by) = match row.state {
            AskState::Answered => (
                State::Answered,
                row.choice.map(action_for),
                row.by.map(By::as_str),
            ),
            AskState::Dismissed => (State::Answered, Some(ActionId::Cancel), None),
            AskState::Expired | AskState::Gone => (State::Withdrawn, None, None),
            _ => return,
        };
        link.inbox.resolve(inbox_id, state, answer, by);
    }
}
