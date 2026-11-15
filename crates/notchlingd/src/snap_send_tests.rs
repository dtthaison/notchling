use crate::snap_testkit::{id_of, Fixture};
use notchling_engine::cluster::ack_book::AckBook;
use notchling_engine::cluster::snapshot::PartKind;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(20);

fn read(fx: &Fixture, node: u8) -> Vec<u8> {
    let store_dir = fx
        .net
        .member(&id_of(node))
        .cluster
        .join(format!("standby/{}/tails/parts", fx.primary));
    let path = store_dir.join(fx.transcript().strip_prefix("/").unwrap());
    std::fs::read(path).unwrap_or_default()
}

fn current(fx: &Fixture, node: u8) -> Option<String> {
    fx.store_of(&id_of(node)).current().map(|m| m.set)
}

#[test]
fn every_ready_can_be_master_machine_gets_the_set() {
    let fx = Fixture::new("send-every", &[(1, true), (2, true)]);
    fx.make_ready(&[id_of(1), id_of(2)]);
    let (manifest, dir) = fx.stage(1, 0, b"one\ntwo\n");
    let targets = fx.sender.offer(manifest.clone(), dir);
    assert_eq!(targets, vec![id_of(1), id_of(2)]);
    assert!(fx.sender.wait_idle(WAIT));
    for n in [1, 2] {
        assert_eq!(fx.store_of(&id_of(n)).current(), Some(manifest.clone()));
        assert_eq!(read(&fx, n), b"one\ntwo\n");
        assert_eq!(fx.sender.sent(&id_of(n)), Some("s-1".to_string()));
    }
    let big = fx.t.path("home/data/pages/img/b.bin");
    let pages = fx.net.member(&id_of(1)).cluster.join(format!(
        "standby/{}/sets/s-1/parts/{}",
        fx.primary,
        big.strip_prefix("/").unwrap().display()
    ));
    assert_eq!(std::fs::read(pages).unwrap(), vec![7; 5000]);
}

#[test]
fn not_ready_machine_gets_nothing() {
    let fx = Fixture::new("send-notready", &[(1, true), (2, true)]);
    fx.make_ready(&[id_of(2)]);
    let (manifest, dir) = fx.stage(1, 0, b"x");
    assert_eq!(fx.sender.offer(manifest, dir), vec![id_of(2)]);
    assert!(fx.sender.wait_idle(WAIT));
    assert_eq!(fx.net.calls(&id_of(1), "snap.offer"), 0);
    assert_eq!(fx.net.calls(&id_of(1), "node.put"), 0);
    assert_eq!(current(&fx, 1), None);
    assert_eq!(current(&fx, 2), Some("s-1".to_string()));
}

#[test]
fn worker_gets_nothing() {
    let fx = Fixture::new("send-worker", &[(1, true), (3, false)]);
    fx.make_ready(&[id_of(1), id_of(3)]);
    let (manifest, dir) = fx.stage(1, 0, b"x");
    assert_eq!(fx.sender.offer(manifest, dir), vec![id_of(1)]);
    assert!(fx.sender.wait_idle(WAIT));
    assert_eq!(fx.net.calls(&id_of(3), "snap.offer"), 0);
    assert_eq!(current(&fx, 3), None);
}

#[test]
fn a_machine_that_is_not_primary_sends_nothing() {
    let fx = Fixture::new("send-nonprimary", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    *fx.net.home.lock().unwrap() = id_of(1);
    let (manifest, dir) = fx.stage(1, 0, b"x");
    assert!(fx.sender.offer(manifest, dir).is_empty());
    assert!(fx.sender.tick().is_empty());
    assert_eq!(fx.net.calls(&id_of(1), "snap.offer"), 0);
}

#[test]
fn newest_queued_set_wins() {
    let fx = Fixture::new("send-newest", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let (m1, d1) = fx.stage(1, 0, b"first\n");
    let (m2, d2) = fx.stage(2, 0, b"first\nsecond\n");
    let (m3, d3) = fx.stage(3, 0, b"first\nsecond\nthird\n");
    fx.net.hold();
    fx.sender.offer(m1, d1);
    fx.net.wait_entered(1);
    fx.sender.offer(m2, d2);
    fx.sender.offer(m3.clone(), d3);
    fx.net.release();
    assert!(fx.sender.wait_idle(WAIT));
    assert_eq!(current(&fx, 1), Some(m3.set.clone()));
    assert_eq!(fx.net.calls(&id_of(1), "snap.offer"), 2);
    assert_eq!(read(&fx, 1), b"first\nsecond\nthird\n");
    let sets = fx
        .net
        .member(&id_of(1))
        .cluster
        .join(format!("standby/{}/sets", fx.primary));
    assert!(!sets.join("s-2").exists());
    assert!(!sets.join("s-2.partial").exists());
}

#[test]
fn the_set_in_flight_is_never_replaced() {
    let fx = Fixture::new("send-inflight", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let (m1, d1) = fx.stage(1, 0, b"first\n");
    let (m2, d2) = fx.stage(2, 0, b"second\n");
    fx.net.hold();
    fx.sender.offer(m1, d1);
    fx.net.wait_entered(1);
    fx.sender.offer(m2, d2);
    assert_eq!(current(&fx, 1), None);
    fx.net.release();
    assert!(fx.sender.wait_idle(WAIT));
    assert_eq!(current(&fx, 1), Some("s-2".to_string()));
    assert!(fx.net.calls(&id_of(1), "snap.commit") >= 2);
}

#[test]
fn ack_records_each_target() {
    let fx = Fixture::new("send-ack", &[(1, true), (2, true)]);
    fx.make_ready(&[id_of(1), id_of(2)]);
    let (manifest, dir) = fx.stage(1, 0, b"one\ntwo\n");
    fx.sender.offer(manifest, dir);
    assert!(fx.sender.wait_idle(WAIT));
    let transcript = fx.transcript();
    for n in [1, 2] {
        let acked = fx.book.get(&id_of(n)).expect("acked");
        assert_eq!(acked.set, "s-1");
        assert_eq!(acked.lengths.get(&transcript).copied(), Some(8));
    }
    assert_eq!(fx.net.calls(&fx.primary, "snap.ack"), 2);
    let reloaded = AckBook::new(&fx.net.member(&fx.primary).cluster);
    assert_eq!(reloaded.all(), fx.book.all());
}

#[test]
fn each_target_is_sent_from_its_own_acknowledged_length() {
    let fx = Fixture::new("send-own", &[(1, true), (2, true)]);
    fx.make_ready(&[id_of(1), id_of(2)]);
    let (a, b) = (id_of(1), id_of(2));
    fx.net.set_online(&a, false);
    let (m1, d1) = fx.stage(1, 0, b"123456");
    assert_eq!(fx.sender.offer(m1, d1), vec![b.clone()]);
    assert!(fx.sender.wait_idle(WAIT));
    fx.net.set_online(&a, true);
    fx.net.set_online(&b, false);
    let (m2, d2) = fx.stage(2, 0, b"123456abcdef");
    assert_eq!(fx.sender.offer(m2, d2), vec![a.clone()]);
    assert!(fx.sender.wait_idle(WAIT));
    fx.net.set_online(&b, true);
    let (m3, d3) = fx.stage(3, 6, b"123456abcdefghijklmn");
    assert_eq!(m3.parts.len(), 3);
    assert_eq!(fx.sender.offer(m3, d3).len(), 2);
    assert!(fx.sender.wait_idle(WAIT));
    for n in [1, 2] {
        assert_eq!(read(&fx, n), b"123456abcdefghijklmn", "node {n}");
        let held = fx.store_of(&id_of(n)).current().expect("current");
        assert_eq!(held.set, "s-3");
    }
    let tail_of = |n: u8| {
        let held = fx.store_of(&id_of(n)).current().unwrap();
        held.parts
            .iter()
            .find_map(|p| match p.kind {
                PartKind::Tail { from, .. } => Some((from, p.len)),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(tail_of(1), (12, 8));
    assert_eq!(tail_of(2), (6, 14));
}

#[test]
fn a_set_ahead_of_the_targets_transcript_is_refused_and_the_current_set_stays() {
    let fx = Fixture::new("send-behind", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let (m1, d1) = fx.stage(1, 0, b"123456");
    fx.sender.offer(m1, d1);
    assert!(fx.sender.wait_idle(WAIT));
    let (m2, d2) = fx.stage(2, 20, b"123456789012345678901234");
    fx.sender.offer(m2, d2);
    assert!(fx.sender.wait_idle(WAIT));
    assert!(fx.sender.last_error(&id_of(1)).is_some());
    assert_eq!(current(&fx, 1), Some("s-1".to_string()));
    assert_eq!(read(&fx, 1), b"123456");
}

#[test]
fn a_target_back_online_gets_the_newest_set() {
    let fx = Fixture::new("send-back", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    fx.net.set_online(&id_of(1), false);
    let (m1, d1) = fx.stage(1, 0, b"one\n");
    assert!(fx.sender.offer(m1, d1).is_empty());
    let (m2, d2) = fx.stage(2, 0, b"one\ntwo\n");
    assert!(fx.sender.offer(m2, d2).is_empty());
    assert!(fx.sender.tick().is_empty());
    fx.net.set_online(&id_of(1), true);
    assert_eq!(fx.sender.tick(), vec![id_of(1)]);
    assert!(fx.sender.wait_idle(WAIT));
    assert_eq!(current(&fx, 1), Some("s-2".to_string()));
    assert!(fx.sender.tick().is_empty());
    assert_eq!(fx.net.calls(&id_of(1), "snap.offer"), 1);
}

#[test]
fn a_cut_transfer_keeps_the_last_complete_set_and_resumes_with_the_missing_parts() {
    let fx = Fixture::new("send-cut", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let a = id_of(1);
    let (m1, d1) = fx.stage(1, 0, b"one\n");
    fx.sender.offer(m1, d1);
    assert!(fx.sender.wait_idle(WAIT));
    let (m2, d2) = fx.stage(2, 0, b"one\ntwo\n");
    fx.net.cut(&a, "snap.part", 1);
    fx.sender.offer(m2, d2);
    assert!(fx.sender.wait_idle(WAIT));
    assert!(fx.sender.last_error(&a).is_some());
    assert_eq!(current(&fx, 1), Some("s-1".to_string()));
    assert_eq!(read(&fx, 1), b"one\n");
    fx.net.mend();
    let before = fx.net.calls(&a, "snap.part");
    assert_eq!(fx.sender.tick(), vec![a.clone()]);
    assert!(fx.sender.wait_idle(WAIT));
    assert_eq!(fx.net.calls(&a, "snap.part") - before, 1);
    assert_eq!(current(&fx, 1), Some("s-2".to_string()));
    assert_eq!(read(&fx, 1), b"one\ntwo\n");
    assert!(fx.sender.last_error(&a).is_none());
}
