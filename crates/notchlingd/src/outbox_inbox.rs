use crate::acting_ops::now_ms;
use crate::inbox_ops::{InboxOps, Responder};
use crate::outbox_ops::OutboxOps;
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use notchling_engine::outbox::Row;
use notchling_events::kinds::agents::OutboxThen;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

pub const NOTICE_AFTER_MS: i64 = 7 * 86_400_000;
pub const SWEEP_EVERY_MS: i64 = 3_600_000;
const LOOK_EVERY: Duration = Duration::from_secs(60);
const NAME_MAX: usize = 64;

enum Msg {
    Answer(String, ActionId),
    Stop,
}

struct Handoff {
    tx: Sender<Msg>,
}

impl Responder for Handoff {
    fn answered(&self, item: &Item, action: ActionId) -> bool {
        match action {
            ActionId::Send | ActionId::Discard => {
                self.tx.send(Msg::Answer(item.id.clone(), action)).is_ok()
            }
            _ => false,
        }
    }
}

pub struct OutboxNotice {
    outbox: Arc<OutboxOps>,
    inbox: Arc<InboxOps>,
    noticed: Mutex<HashMap<String, Vec<String>>>,
    tx: Sender<Msg>,
    rx: Mutex<Receiver<Msg>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(NAME_MAX)
        .collect::<String>()
        .trim()
        .to_string()
}

fn detail(count: usize, name: &str) -> String {
    match count {
        1 => format!("1 message to {name} hasn't gone out for 7 days."),
        n => format!("{n} messages to {name} haven't gone out for 7 days."),
    }
}

fn by_session(rows: Vec<Row>) -> Vec<(String, Vec<Row>)> {
    let mut groups: Vec<(String, Vec<Row>)> = Vec::new();
    for row in rows {
        match groups
            .iter_mut()
            .find(|(session, _)| *session == row.session)
        {
            Some((_, group)) => group.push(row),
            None => groups.push((row.session.clone(), vec![row])),
        }
    }
    groups
}

impl OutboxNotice {
    pub fn new(outbox: Arc<OutboxOps>, inbox: Arc<InboxOps>) -> Arc<OutboxNotice> {
        let (tx, rx) = channel();
        Arc::new(OutboxNotice {
            outbox,
            inbox,
            noticed: Mutex::new(HashMap::new()),
            tx,
            rx: Mutex::new(rx),
            thread: Mutex::new(None),
        })
    }

    pub fn sweep(&self, now_ms: i64) {
        self.withdraw_gone();
        let store = self.outbox.store();
        let Ok(rows) = store.unnoticed_before(now_ms - NOTICE_AFTER_MS) else {
            return;
        };
        for (session, rows) in by_session(rows) {
            let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
            if store.mark_noticed(&ids, now_ms).is_err() {
                continue;
            }
            let name = self
                .outbox
                .session_name(&session)
                .map(|n| plain(&n))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "the session".to_string());
            let id = format!("outbox:{session}:{now_ms}");
            let item = Item {
                id: id.clone(),
                kind: Kind::OutboxUnsent,
                title: "Messages Not Sent".to_string(),
                detail: detail(rows.len(), &name),
                source: Source {
                    node: None,
                    session: Some(session),
                    job: None,
                },
                created_ms: now_ms,
                expires_ms: None,
                actions: vec![
                    Action {
                        id: ActionId::Send,
                        label: "Send".to_string(),
                        primary: true,
                    },
                    Action {
                        id: ActionId::Discard,
                        label: "Discard".to_string(),
                        primary: false,
                    },
                ],
                state: State::Open,
                answer: None,
                answered_by: None,
                seen: false,
                open: None,
                closed_ms: None,
            };
            lock(&self.noticed).insert(id, ids);
            let responder = Arc::new(Handoff {
                tx: self.tx.clone(),
            });
            self.inbox.raise(item, Some(responder));
        }
    }

    fn withdraw_gone(&self) {
        let open: Vec<(String, Vec<String>)> = lock(&self.noticed)
            .iter()
            .map(|(id, ids)| (id.clone(), ids.clone()))
            .collect();
        for (item, ids) in open {
            let store = self.outbox.store();
            let any = ids
                .iter()
                .any(|id| matches!(store.get(id), Ok(Some(_)) | Err(_)));
            if !any {
                lock(&self.noticed).remove(&item);
                self.inbox.resolve(&item, State::Withdrawn, None, None);
            }
        }
    }

    pub fn drain(&self) {
        loop {
            let next = lock(&self.rx).try_recv();
            match next {
                Ok(Msg::Answer(item, action)) => self.apply(&item, action),
                Ok(Msg::Stop) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            }
        }
    }

    fn apply(&self, item: &str, action: ActionId) {
        let Some(ids) = lock(&self.noticed).remove(item) else {
            return;
        };
        match action {
            ActionId::Send => {
                for id in &ids {
                    let _ = self.outbox.resend_row(id);
                }
            }
            ActionId::Discard => {
                let Ok(rows) = self.outbox.store().discard_rows(&ids) else {
                    return;
                };
                for row in &rows {
                    self.outbox
                        .publish_removed(&row.id, &row.session, "discarded");
                    self.outbox.emit_then(row, OutboxThen::Discarded);
                }
            }
            _ => {}
        }
    }

    pub fn spawn_loop(self: &Arc<Self>) {
        let mut thread = lock(&self.thread);
        if thread.is_some() {
            return;
        }
        let notice = self.clone();
        *thread = std::thread::Builder::new()
            .name("notchling-outbox-notice".into())
            .spawn(move || notice.run())
            .ok();
    }

    fn run(&self) {
        let mut swept = now_ms();
        self.sweep(swept);
        loop {
            let next = lock(&self.rx).recv_timeout(LOOK_EVERY);
            match next {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Msg::Answer(item, action)) => self.apply(&item, action),
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = now_ms();
            if now - swept >= SWEEP_EVERY_MS {
                swept = now;
                self.sweep(now);
            } else {
                self.withdraw_gone();
            }
        }
    }

    pub fn close(&self) {
        let _ = self.tx.send(Msg::Stop);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
#[path = "outbox_inbox_tests.rs"]
mod tests;
