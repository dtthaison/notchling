use crate::acting_ops_tests::FakeRoom;
use crate::cluster_move_tests::{logged, one, origin, pair, run, takeover, Fault, Pair};
use crate::inbox_ops::{InboxOps, Responder};
use crate::inbox_ops_tests::item;
use crate::snap_restore_testkit::{World, S1};
use notchling_engine::inbox::{self, ActionId, Item, Kind, State, Store};
use notchling_local_api::ApiError;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

struct Counted(Arc<AtomicUsize>);

impl Responder for Counted {
    fn answered(&self, _item: &Item, _action: ActionId) -> bool {
        self.0.fetch_add(1, SeqCst);
        true
    }
}

fn switch_to_b(p: &Pair) -> Result<Value, ApiError> {
    let ask = takeover(&p.a, &p.b, 2, &[S1]);
    p.to.mover
        .switch_here(&serde_json::to_value(&ask).unwrap(), &origin(&p.a))
}

fn raised_before_the_set(w: &World) {
    let theirs = InboxOps::new(w.at("theirs"), Arc::new(FakeRoom::default()))
        .with_cluster_dir(w.cluster.clone());
    theirs.load();
    theirs.raise(item("from-item"), None);
    theirs.flush();
    theirs.close();
}

fn own_inbox(p: &Pair) -> (Arc<InboxOps>, Arc<AtomicUsize>) {
    let own = p.to.inbox.clone().with_cluster_dir(p.w.cluster.clone());
    own.load();
    let calls = Arc::new(AtomicUsize::new(0));
    own.raise(item("to-own"), Some(Arc::new(Counted(calls.clone()))));
    own.flush();
    (own, calls)
}

fn state_of(inbox: &InboxOps, id: &str) -> Option<State> {
    inbox
        .items()
        .into_iter()
        .find(|i| i.id == id)
        .map(|i| i.state)
}

fn saved_ids(p: &Pair) -> Vec<String> {
    inbox::load_from(&Store::Cluster(p.w.cluster.clone()), 0)
        .expect("the inbox reads")
        .items()
        .into_iter()
        .map(|i| i.id)
        .collect()
}

#[test]
fn r2_a_takeover_keeps_the_restored_open_items_and_the_live_responders() {
    let p = pair(&one(), &raised_before_the_set);
    let (own, calls) = own_inbox(&p);

    switch_to_b(&p).expect("the switch");

    assert_eq!(state_of(&own, "from-item"), Some(State::Open));
    assert_eq!(state_of(&own, "to-own"), Some(State::Open));
    own.decide("to-own", ActionId::Allow, "mac")
        .expect("answered");
    assert_eq!(calls.load(SeqCst), 1, "the responder was kept");
    own.flush();
    let saved = saved_ids(&p);
    assert!(saved.contains(&"to-own".to_string()), "{saved:?}");
    assert!(saved.contains(&"from-item".to_string()), "{saved:?}");
}

#[test]
fn r2_a_failed_takeover_leaves_the_inbox_untouched() {
    let p = pair(&one(), &|_| {});
    let (own, calls) = own_inbox(&p);
    let before = own.items();
    *p.to.link.take_fails.lock().unwrap() = Some("the core said no".to_string());

    switch_to_b(&p).unwrap_err();

    assert_eq!(own.items(), before);
    own.decide("to-own", ActionId::Allow, "mac")
        .expect("answered");
    assert_eq!(calls.load(SeqCst), 1, "the responder was kept");
}

#[test]
fn r2_an_item_raised_while_the_database_is_swapped_is_kept() {
    let p = pair(&one(), &|_| {});
    let (own, _) = own_inbox(&p);
    let during = own.clone();
    *p.to.link.on_take.lock().unwrap() = Some(Box::new(move |_| {
        during.raise(item("mid-swap"), None);
        during.flush();
    }));

    switch_to_b(&p).expect("the switch");

    assert_eq!(state_of(&own, "mid-swap"), Some(State::Open));
    own.flush();
    let saved = saved_ids(&p);
    assert!(saved.contains(&"mid-swap".to_string()), "{saved:?}");
}

#[test]
fn r2_a_resume_retry_waits_for_the_one_in_flight() {
    let p = pair(&one(), &|_| {});
    let (entered, enter) = mpsc::channel::<()>();
    let entered = Mutex::new(entered);
    *p.to.acts.on_start.lock().unwrap() = Some(Box::new(move |_| {
        let _ = entered.lock().unwrap().send(());
        std::thread::sleep(Duration::from_millis(400));
    }));
    *p.from.link.entered.lock().unwrap() = Some(enter);
    p.from
        .link
        .call_faults
        .lock()
        .unwrap()
        .push_back(Fault::DropEarly);

    let runner = run(&p);
    for late in std::mem::take(&mut *p.from.link.late.lock().unwrap()) {
        late.join().unwrap();
    }

    assert_eq!(runner.status()["step"], "done", "{}", runner.status());
    let failures: Vec<String> = p
        .from
        .inbox
        .items()
        .into_iter()
        .filter(|i| i.kind == Kind::ActionFailed)
        .map(|i| i.detail)
        .collect();
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(
        logged(&p),
        vec![
            format!("from stop {S1}"),
            format!("to start {S1} --name tester --model x"),
        ]
    );
}
