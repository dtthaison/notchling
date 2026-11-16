use crate::cluster_promote::Cluster;
use crate::cluster_promote_kit::{rig, Rig, BOOT_MS};
use crate::cluster_promote_watch::{
    clock_text, WatchThread, Watcher, NO_PRIMARY_AFTER_MS, UNREACHABLE_AFTER_MS,
};
use notchling_engine::inbox::{ActionId, Item, Kind, State};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

fn watch(r: &Rig) -> Arc<Watcher> {
    let cluster = r.cluster.clone();
    Watcher::with_offset(
        r.promoter.clone(),
        Arc::new(move || Cluster::status(&*cluster)),
        Arc::new(|_| 7200),
    )
}

fn home_online(r: &Rig, online: bool) {
    let home = r.a();
    r.p.to.link.edit(|v| {
        for m in v["members"].as_array_mut().unwrap() {
            if m["id"] == json!(home) {
                m["online"] = json!(online);
            }
        }
    });
}

fn me_as_client(r: &Rig) {
    let me = r.b();
    r.p.to.link.edit(|v| {
        for m in v["members"].as_array_mut().unwrap() {
            if m["id"] == json!(me) {
                m["canBeMaster"] = json!(false);
                m["client"] = json!(true);
            }
        }
    });
}

fn at(r: &Rig, after_ms: u64) {
    r.clock.store(BOOT_MS + after_ms, Ordering::SeqCst);
}

fn open(r: &Rig, id: &str) -> Option<Item> {
    r.item(id).filter(|i| i.state == State::Open)
}

const UNREACHABLE: &str = "primary-unreachable:1:5000000";
const CONFIRM: &str = "primary-unreachable:1:5000000:confirm";
const NO_PRIMARY: &str = "no-primary:1:5000000";

#[test]
fn nothing_is_raised_before_a_minute() {
    let r = rig();
    home_online(&r, false);
    let w = watch(&r);
    w.tick();
    at(&r, UNREACHABLE_AFTER_MS - 1);
    w.tick();
    assert!(r.open_ids().is_empty());
    at(&r, UNREACHABLE_AFTER_MS);
    w.tick();
    let item = open(&r, UNREACHABLE).expect("the item");
    assert_eq!(item.kind, Kind::PrimaryUnreachable);
    assert_eq!(item.title, "m1 hasn't answered for 1 minute");
    assert_eq!(item.actions.len(), 1);
    assert_eq!(item.actions[0].id, ActionId::MoveHere);
    assert_eq!(item.actions[0].label, "Move to mine Now");
}

#[test]
fn the_takeover_time_is_in_the_detail() {
    let r = rig();
    home_online(&r, false);
    let (me, wall) = (r.b(), 1_700_000_000_000i64);
    r.p.to.link.edit(|v| {
        v["failover"] = json!({"value": "30m", "armed": true, "target": me, "takesOverAt": wall});
    });
    let w = watch(&r);
    w.tick();
    at(&r, UNREACHABLE_AFTER_MS);
    w.tick();
    assert_eq!(clock_text(wall, 7200), "00:13");
    let item = open(&r, UNREACHABLE).unwrap();
    assert_eq!(item.detail, "mine takes over at 00:13.");
}

#[test]
fn no_takeover_time_when_failover_is_off_or_not_armed() {
    let r = rig();
    home_online(&r, false);
    let me = r.b();
    r.p.to.link.edit(|v| {
        v["failover"] = json!({"value": "off", "armed": true, "target": me, "takesOverAt": 5});
    });
    let w = watch(&r);
    w.tick();
    at(&r, UNREACHABLE_AFTER_MS);
    w.tick();
    assert_eq!(open(&r, UNREACHABLE).unwrap().detail, "");
}

#[test]
fn the_item_is_raised_once_and_withdrawn_when_the_primary_returns() {
    let r = rig();
    home_online(&r, false);
    let w = watch(&r);
    w.tick();
    at(&r, UNREACHABLE_AFTER_MS);
    w.tick();
    w.tick();
    assert_eq!(r.kinds(Kind::PrimaryUnreachable).len(), 1);
    home_online(&r, true);
    w.tick();
    assert_eq!(r.item(UNREACHABLE).unwrap().state, State::Withdrawn);
    home_online(&r, false);
    at(&r, UNREACHABLE_AFTER_MS + 1_000);
    w.tick();
    assert!(r.open_ids().is_empty());
    at(&r, 2 * UNREACHABLE_AFTER_MS + 1_000);
    w.tick();
    let second = format!(
        "primary-unreachable:1:{}",
        BOOT_MS + UNREACHABLE_AFTER_MS + 1_000
    );
    assert!(open(&r, &second).is_some());
    assert_eq!(r.item(UNREACHABLE).unwrap().state, State::Withdrawn);
    assert_eq!(r.kinds(Kind::PrimaryUnreachable).len(), 2);
}

#[test]
fn a_peer_that_reaches_the_primary_keeps_it_quiet() {
    let r = rig();
    home_online(&r, false);
    let c = crate::snap_testkit::id_of(3);
    r.add_peer(&c, "m2");
    r.answer(&c, "node.reach", Ok(json!({"online": true})));
    let w = watch(&r);
    w.tick();
    at(&r, 5 * UNREACHABLE_AFTER_MS);
    w.tick();
    assert!(r.open_ids().is_empty());
}

#[test]
fn a_peer_that_cannot_reach_it_either_changes_nothing() {
    let r = rig();
    home_online(&r, false);
    let c = crate::snap_testkit::id_of(3);
    r.add_peer(&c, "m2");
    r.answer(&c, "node.reach", Ok(json!({"online": false})));
    let w = watch(&r);
    w.tick();
    at(&r, UNREACHABLE_AFTER_MS);
    w.tick();
    assert!(open(&r, UNREACHABLE).is_some());
}

#[test]
fn a_client_hears_after_two_minutes() {
    let r = rig();
    home_online(&r, false);
    me_as_client(&r);
    let w = watch(&r);
    w.tick();
    at(&r, UNREACHABLE_AFTER_MS);
    w.tick();
    assert!(r.open_ids().is_empty());
    at(&r, NO_PRIMARY_AFTER_MS - 1);
    w.tick();
    assert!(r.open_ids().is_empty());
    at(&r, NO_PRIMARY_AFTER_MS);
    w.tick();
    let item = open(&r, NO_PRIMARY).expect("the item");
    assert_eq!(item.kind, Kind::NoPrimary);
    assert!(item.actions.is_empty());
    home_online(&r, true);
    w.tick();
    assert_eq!(r.item(NO_PRIMARY).unwrap().state, State::Withdrawn);
}

#[test]
fn the_primary_itself_raises_nothing() {
    let r = rig();
    let home = r.a();
    r.p.to.link.edit(|v| v["me"] = json!(home));
    let w = watch(&r);
    w.tick();
    at(&r, 10 * UNREACHABLE_AFTER_MS);
    w.tick();
    assert!(r.open_ids().is_empty());
}

fn raised(r: &Rig) -> Arc<Watcher> {
    home_online(r, false);
    let w = watch(r);
    w.tick();
    at(r, UNREACHABLE_AFTER_MS);
    w.tick();
    w
}

#[test]
fn move_now_asks_for_confirmation_when_the_primary_is_down() {
    let r = rig();
    let _w = raised(&r);
    let outcome =
        r.p.to
            .inbox
            .decide(UNREACHABLE, ActionId::MoveHere, "mac")
            .unwrap();
    assert!(outcome.answered);
    crate::cluster_testkit::eventually("the confirm item", || open(&r, CONFIRM).is_some());
    let item = open(&r, CONFIRM).unwrap();
    assert_eq!(item.actions.len(), 1);
    assert_eq!(item.actions[0].id, ActionId::MoveAnyway);
    assert_eq!(item.actions[0].label, "Move Anyway");
    assert!(item.detail.contains("can't be reached"), "{}", item.detail);
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
}

#[test]
fn move_anyway_runs_the_promote() {
    let r = rig();
    let _w = raised(&r);
    r.p.to
        .inbox
        .decide(UNREACHABLE, ActionId::MoveHere, "mac")
        .unwrap();
    crate::cluster_testkit::eventually("the confirm item", || open(&r, CONFIRM).is_some());
    let outcome =
        r.p.to
            .inbox
            .decide(CONFIRM, ActionId::MoveAnyway, "mac")
            .unwrap();
    assert!(outcome.answered);
    crate::cluster_testkit::eventually("the term moves", || {
        !r.p.to.link.took.lock().unwrap().is_empty()
    });
    r.finish();
    assert_eq!(r.p.to.link.took.lock().unwrap().len(), 1);
}

#[test]
fn nothing_moves_while_acting_is_off() {
    let r = rig();
    let _w = raised(&r);
    r.acting.set(false);
    let outcome =
        r.p.to
            .inbox
            .decide(UNREACHABLE, ActionId::MoveHere, "phone")
            .unwrap();
    assert!(!outcome.answered);
    assert!(open(&r, UNREACHABLE).is_some());
    std::thread::sleep(Duration::from_millis(100));
    assert!(open(&r, CONFIRM).is_none());
    assert!(!r.promoter.running());
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
}

#[test]
fn the_confirm_item_answers_only_move_anyway() {
    let r = rig();
    let _w = raised(&r);
    r.p.to
        .inbox
        .decide(UNREACHABLE, ActionId::MoveHere, "mac")
        .unwrap();
    crate::cluster_testkit::eventually("the confirm item", || open(&r, CONFIRM).is_some());
    let outcome = r.p.to.inbox.decide(CONFIRM, ActionId::MoveHere, "mac");
    assert!(outcome.is_err() || !outcome.unwrap().answered);
    assert!(open(&r, CONFIRM).is_some());
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
}

#[test]
fn the_thread_ticks_and_closes() {
    let r = rig();
    home_online(&r, false);
    let w = watch(&r);
    let thread = WatchThread::spawn(w, Duration::from_millis(10));
    std::thread::sleep(Duration::from_millis(60));
    at(&r, UNREACHABLE_AFTER_MS);
    crate::cluster_testkit::eventually("the item", || open(&r, UNREACHABLE).is_some());
    thread.close();
    thread.close();
}

#[test]
fn the_confirm_item_is_withdrawn_when_the_primary_returns() {
    let r = rig();
    let w = raised(&r);
    r.p.to
        .inbox
        .decide(UNREACHABLE, ActionId::MoveHere, "mac")
        .unwrap();
    crate::cluster_testkit::eventually("the confirm item", || open(&r, CONFIRM).is_some());
    r.finish();
    home_online(&r, true);
    w.tick();
    assert!(open(&r, CONFIRM).is_none());
}

#[test]
fn a_second_outage_in_the_same_term_raises_a_new_confirm_item() {
    let r = rig();
    let w = raised(&r);
    r.p.to
        .inbox
        .decide(UNREACHABLE, ActionId::MoveHere, "mac")
        .unwrap();
    crate::cluster_testkit::eventually("the confirm item", || open(&r, CONFIRM).is_some());
    r.finish();
    home_online(&r, true);
    w.tick();
    assert!(open(&r, CONFIRM).is_none());
    home_online(&r, false);
    at(&r, UNREACHABLE_AFTER_MS + 1_000);
    w.tick();
    at(&r, 2 * UNREACHABLE_AFTER_MS + 1_000);
    w.tick();
    let second = format!(
        "primary-unreachable:1:{}",
        BOOT_MS + UNREACHABLE_AFTER_MS + 1_000
    );
    r.p.to
        .inbox
        .decide(&second, ActionId::MoveHere, "mac")
        .unwrap();
    let confirm = format!("{second}:confirm");
    crate::cluster_testkit::eventually("the second confirm item", || open(&r, &confirm).is_some());
}

#[test]
fn a_client_hears_again_about_a_second_outage() {
    let r = rig();
    home_online(&r, false);
    me_as_client(&r);
    let w = watch(&r);
    w.tick();
    at(&r, NO_PRIMARY_AFTER_MS);
    w.tick();
    assert!(open(&r, NO_PRIMARY).is_some());
    home_online(&r, true);
    w.tick();
    home_online(&r, false);
    at(&r, NO_PRIMARY_AFTER_MS + 1_000);
    w.tick();
    at(&r, 2 * NO_PRIMARY_AFTER_MS + 1_000);
    w.tick();
    let second = format!("no-primary:1:{}", BOOT_MS + NO_PRIMARY_AFTER_MS + 1_000);
    assert!(open(&r, &second).is_some());
    assert_eq!(r.item(NO_PRIMARY).unwrap().state, State::Withdrawn);
}

#[test]
fn an_item_open_before_a_restart_is_withdrawn_when_the_primary_is_back() {
    let r = rig();
    let _before = raised(&r);
    assert!(open(&r, UNREACHABLE).is_some());
    let after = watch(&r);
    home_online(&r, true);
    after.tick();
    assert_eq!(r.item(UNREACHABLE).unwrap().state, State::Withdrawn);
    assert!(r.open_ids().is_empty());
}

#[test]
fn an_item_open_before_a_restart_is_withdrawn_when_a_new_outage_begins() {
    let r = rig();
    let _before = raised(&r);
    r.p.to
        .inbox
        .decide(UNREACHABLE, ActionId::MoveHere, "mac")
        .unwrap();
    crate::cluster_testkit::eventually("the confirm item", || open(&r, CONFIRM).is_some());
    r.finish();
    let after = watch(&r);
    assert!(open(&r, CONFIRM).is_some());
    after.tick();
    assert!(open(&r, CONFIRM).is_none());
    assert!(r.open_ids().is_empty());
    at(&r, 2 * UNREACHABLE_AFTER_MS);
    after.tick();
    let fresh = format!("primary-unreachable:1:{}", BOOT_MS + UNREACHABLE_AFTER_MS);
    assert!(open(&r, &fresh).is_some());
}
