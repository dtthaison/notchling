use super::*;
use crate::inbox_ops_tests::{rig as inbox_rig, Rig as InboxRig};
use crate::outbox_testkit::{rig, Rig, IDLE, T0};
use notchling_engine::inbox::{ActionId, Kind, State as ItemState};
use notchling_engine::outbox::{New, State, Tool, Via, Why};
use std::sync::atomic::{AtomicUsize, Ordering};

const DAY: i64 = 86_400_000;
const OLD: i64 = T0 - 8 * DAY;
const TOKEN: &str = "zq7-planted-token";

struct Env {
    r: Rig,
    inbox: InboxRig,
    notice: Arc<OutboxNotice>,
    resumed: Arc<AtomicUsize>,
}

fn env() -> Env {
    let r = rig();
    let inbox = inbox_rig();
    let notice = OutboxNotice::new(r.ops.clone(), inbox.ops.clone());
    let resumed = Arc::new(AtomicUsize::new(0));
    let count = resumed.clone();
    r.ops.set_resume(Arc::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    Env {
        r,
        inbox,
        notice,
        resumed,
    }
}

impl Env {
    fn add(&self, session: &str, text: &str, queued_ms: i64) -> String {
        let new = New {
            session: session.into(),
            tool: Tool::Claude,
            text: text.into(),
            options: None,
            via: Via::Chat,
            clear_box: false,
        };
        self.r.ops.store().insert(&new, queued_ms).unwrap().id
    }

    fn put(&self, id: &str, from: State, to: State, why: Option<Why>) {
        let moved = self
            .r
            .ops
            .store()
            .set_state(id, &[from], to, why, None, OLD)
            .unwrap();
        assert!(moved);
    }

    fn state(&self, id: &str) -> Option<(State, Option<Why>, bool)> {
        self.r
            .ops
            .store()
            .get(id)
            .unwrap()
            .map(|row| (row.state, row.why, row.clear_box))
    }

    fn items(&self) -> Vec<Item> {
        self.inbox.ops.items()
    }

    fn answer(&self, action: ActionId) {
        let id = self.items()[0].id.clone();
        let outcome = self.inbox.ops.decide(&id, action, "mac").unwrap();
        assert!(outcome.answered);
        self.notice.drain();
    }

    fn five_kinds(&self) -> [String; 5] {
        let ids = [
            self.add(IDLE, "unsure", OLD),
            self.add(IDLE, "box", OLD),
            self.add(IDLE, "stopped", OLD),
            self.add(IDLE, "gone", OLD),
            self.add(IDLE, "in queue", OLD),
        ];
        let from = State::Queued;
        self.put(&ids[0], from, State::Unsure, Some(Why::NotFound));
        self.put(&ids[1], from, State::Box, Some(Why::NotCleared));
        self.put(&ids[2], from, State::Stopped, None);
        self.put(&ids[3], from, State::Gone, None);
        self.put(&ids[4], from, State::InQueue, None);
        ids
    }
}

#[test]
fn old_rows_raise_one_item_per_session() {
    let e = env();
    e.add(IDLE, "one", OLD);
    e.add(IDLE, "two", OLD);
    e.add(IDLE, "three", OLD);
    e.add(IDLE, "fresh", T0 - DAY);
    e.add("abcd1234", "other session", OLD);
    e.notice.sweep(T0);
    let items = e.items();
    assert_eq!(items.len(), 2);
    let idle = items
        .iter()
        .find(|i| i.source.session.as_deref() == Some(IDLE))
        .unwrap();
    assert_eq!(idle.id, format!("outbox:{IDLE}:{T0}"));
    assert_eq!(idle.kind, Kind::OutboxUnsent);
    assert_eq!(idle.title, "Messages Not Sent");
    assert_eq!(idle.detail, "3 messages to n haven't gone out for 7 days.");
    assert_eq!(idle.expires_ms, None);
    let actions: Vec<_> = idle.actions.iter().map(|a| (a.id, a.primary)).collect();
    assert_eq!(
        actions,
        vec![(ActionId::Send, true), (ActionId::Discard, false)]
    );
    let other = items.iter().find(|i| i.id != idle.id).unwrap();
    assert_eq!(other.detail, "1 message to n hasn't gone out for 7 days.");
}

#[test]
fn item_has_no_message_text() {
    let e = env();
    e.add(IDLE, &format!("please {TOKEN}"), OLD);
    e.notice.sweep(T0);
    let json = serde_json::to_string(&e.items()).unwrap();
    assert!(!json.contains(TOKEN));
    e.inbox.ops.flush();
    let published = e.inbox.room.named("inbox.changed");
    assert!(!published.is_empty());
    assert!(!serde_json::to_string(&published).unwrap().contains(TOKEN));
}

#[test]
fn raised_once_not_again_after_restart() {
    let e = env();
    e.add(IDLE, "one", OLD);
    e.notice.sweep(T0);
    e.notice.sweep(T0 + 3_600_000);
    assert_eq!(e.items().len(), 1);
    let again = OutboxNotice::new(e.r.ops.clone(), e.inbox.ops.clone());
    again.sweep(T0 + 7_200_000);
    assert_eq!(e.items().len(), 1);
    let row = e.r.ops.store().list(None).unwrap().remove(0);
    assert!(row.noticed_ms.is_some());
}

#[test]
fn send_applies_each_rows_own_action() {
    let e = env();
    let ids = e.five_kinds();
    let queued = e.add(IDLE, "later", OLD);
    e.notice.sweep(T0);
    assert_eq!(e.items().len(), 1);
    e.answer(ActionId::Send);
    assert_eq!(e.state(&ids[0]), Some((State::Queued, None, false)));
    assert_eq!(e.state(&ids[1]), Some((State::Queued, None, true)));
    assert_eq!(e.state(&ids[2]), Some((State::Queued, None, false)));
    assert_eq!(e.resumed.load(Ordering::SeqCst), 1);
    assert_eq!(e.state(&ids[3]).unwrap().0, State::Gone);
    assert_eq!(e.state(&ids[4]).unwrap().0, State::InQueue);
    assert_eq!(e.state(&queued).unwrap().0, State::Queued);
    let item = e.items().remove(0);
    assert_eq!(item.state, ItemState::Answered);
    assert_eq!(item.answer, Some(ActionId::Send));
}

#[test]
fn in_queue_rows_are_counted() {
    let e = env();
    let id = e.add(IDLE, "only one", OLD);
    e.put(&id, State::Queued, State::InQueue, None);
    e.notice.sweep(T0);
    let items = e.items();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].detail,
        "1 message to n hasn't gone out for 7 days."
    );
}

#[test]
fn discard_removes_noticed_rows() {
    let e = env();
    let ids = e.five_kinds();
    let fresh = e.add(IDLE, "fresh", T0 - DAY);
    let sending = e.add(IDLE, "typing", OLD);
    e.notice.sweep(T0);
    e.r.ops.store().take(&sending, T0).unwrap();
    e.answer(ActionId::Discard);
    for id in &ids {
        assert_eq!(e.state(id), None);
    }
    assert!(e.state(&fresh).is_some());
    assert!(e.state(&sending).is_some());
    let gone = e.r.room.named("outbox.removed");
    assert_eq!(gone.len(), 5);
    assert!(gone.iter().all(|g| g["why"] == "discarded"));
    assert_eq!(e.items()[0].answer, Some(ActionId::Discard));
}

#[test]
fn item_withdrawn_when_rows_all_gone() {
    let e = env();
    let a = e.add(IDLE, "one", OLD);
    let b = e.add(IDLE, "two", OLD);
    e.notice.sweep(T0);
    e.r.ops.store().delete(&a).unwrap();
    e.notice.sweep(T0 + 1_000);
    assert_eq!(e.items()[0].state, ItemState::Open);
    e.r.ops.store().delete(&b).unwrap();
    e.notice.sweep(T0 + 2_000);
    assert_eq!(e.items()[0].state, ItemState::Withdrawn);
}
