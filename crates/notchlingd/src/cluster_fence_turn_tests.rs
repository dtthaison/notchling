use crate::cluster_fence_keeper::{Boot, Feed, Keeper, KeeperParts};
use crate::cluster_fence_keeper_kit::{node, FakeFeed, BOOT, S1};
use crate::cluster_move::Mover;
use crate::cluster_returns_kit::{kit, seed_after_promote, KActs, Kit};
use notchling_engine::cluster::fence::Gate;
use notchling_engine::cluster::fence_gates::SetView;
use notchling_engine::cluster::NodeId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Rows = Vec<(String, Result<(), String>)>;

struct BlockFeed {
    inner: Arc<FakeFeed>,
    entered: Mutex<Option<Sender<()>>>,
    release: Mutex<Option<Receiver<()>>>,
}

impl Feed for BlockFeed {
    fn me(&self) -> Option<NodeId> {
        let entered = self.entered.lock().unwrap().take();
        let release = self.release.lock().unwrap().take();
        if let (Some(entered), Some(release)) = (entered, release) {
            let _ = entered.send(());
            let _ = release.recv_timeout(Duration::from_secs(20));
        }
        self.inner.me()
    }

    fn ready(&self) -> BTreeSet<NodeId> {
        self.inner.ready()
    }

    fn latest_set(&self) -> Option<SetView> {
        self.inner.latest_set()
    }

    fn acked(&self, node: &NodeId) -> Option<String> {
        self.inner.acked(node)
    }

    fn waiting(&self) -> BTreeMap<String, String> {
        self.inner.waiting()
    }

    fn live(&self, id: &str) -> bool {
        self.inner.live(id)
    }
}

fn interleave(tag: &str, start: fn(&Mover) -> Rows) -> Kit {
    let k = kit(tag, seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 40);
    k.w.keeper.tick();
    assert_eq!(k.w.gate(S1), Some(Gate::Moved { to: "m1".into() }));
    k.w.feed.live.lock().unwrap().insert(S1.to_string());
    let (entered, entered_rx) = channel();
    let (release_tx, release) = channel();
    let keeper = Keeper::new(
        k.w.cluster.clone(),
        KeeperParts {
            feed: Arc::new(BlockFeed {
                inner: k.w.feed.clone(),
                entered: Mutex::new(Some(entered)),
                release: Mutex::new(Some(release)),
            }),
            acts: Arc::new(KActs { log: k.log.clone() }),
            inbox: k.w.inbox.clone(),
            boot: Boot {
                id: BOOT.to_string(),
                now: Arc::new(|| 1_000),
            },
        },
    );
    let tick = std::thread::spawn(move || keeper.tick());
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the tick reached its feed");
    let (done, done_rx) = channel();
    let mover = k.mover.clone();
    let started = std::thread::spawn(move || {
        let _ = done.send(start(&mover));
    });
    assert!(
        done_rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "the start ran while a keeper tick that read the older state was in flight"
    );
    release_tx.send(()).unwrap();
    tick.join().unwrap();
    let rows = done_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the start finished");
    started.join().unwrap();
    assert!(rows[0].1.is_ok(), "{rows:?}");
    k
}

fn assert_started_after_the_tick(k: &Kit) {
    let fence = k.w.fence();
    assert!(
        fence
            .sessions
            .values()
            .all(|gate| !matches!(gate, Gate::Moved { .. })),
        "{fence:?}"
    );
    let log = k.logged();
    let stop = log.iter().position(|line| *line == format!("stop {S1}"));
    let start = log.iter().position(|line| *line == format!("start {S1}"));
    assert!(stop.is_some() && start.is_some(), "{log:?}");
    assert!(stop < start, "{log:?}");
    k.w.keeper.tick();
    assert_eq!(k.w.gate(S1), None);
    assert_eq!(
        k.logged().iter().filter(|l| l.starts_with("stop")).count(),
        1
    );
}

#[test]
fn a_keeper_tick_with_older_state_ends_before_a_start_here_lifts_the_gate() {
    let k = interleave("turn-start", |mover| mover.start_here(&[S1.to_string()]));
    assert_started_after_the_tick(&k);
}

#[test]
fn a_keeper_tick_with_older_state_ends_before_a_restart_lifts_the_gate() {
    let k = interleave("turn-restart", |mover| {
        mover.restart(&[S1.to_string()], &[])
    });
    assert_started_after_the_tick(&k);
}

#[test]
fn a_start_waits_for_its_claim_to_reach_this_machine() {
    let k = kit("turn-lag", seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 40);
    k.w.keeper.tick();
    *k.link.lag.lock().unwrap() = Some(Duration::from_millis(250));
    let rows = k.mover.start_here(&[S1.to_string()]);
    assert!(rows[0].1.is_ok(), "{rows:?}");
    assert_eq!(k.state().claims[S1].node, node("m5"));
    assert_eq!(
        k.logged(),
        vec![format!("record claim {S1} 2"), format!("start {S1}")]
    );
    k.w.keeper.tick();
    assert_eq!(k.w.gate(S1), None);
}

#[test]
fn a_claim_that_never_reaches_this_machine_starts_nothing() {
    let k = kit("turn-lost", seed_after_promote(&[(S1, 1)]));
    let why = k
        .mover
        .wait_local(S1, &node("m5"), Duration::from_millis(60))
        .unwrap_err();
    assert!(why.contains("hasn't reached this machine"), "{why}");
    assert!(k.logged().is_empty());
}
