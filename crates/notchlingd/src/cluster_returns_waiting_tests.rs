use crate::cluster_fence_keeper_kit::{node, world, Seed, S1};
use crate::cluster_returns_waiting::{Waiting, WAITING_FILE};
use notchling_engine::cluster::fence::Gate;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn waiting_session_gets_a_waiting_gate() {
    let w = world("waiting-gate", Seed::default());
    let waiting = Waiting::new(&w.cluster);
    waiting.add(S1, &node("m1"), "m1").unwrap();
    *w.feed.waiting.lock().unwrap() = waiting.gates();
    w.keeper.tick();
    assert_eq!(
        w.gate(S1),
        Some(Gate::Waiting {
            for_node: "m1".into()
        })
    );
    waiting.remove(S1).unwrap();
    *w.feed.waiting.lock().unwrap() = waiting.gates();
    w.keeper.tick();
    assert_eq!(w.gate(S1), Some(Gate::Free));
}

#[test]
fn damaged_waiting_file_reads_empty_and_is_reported() {
    let w = world("waiting-damaged", Seed::default());
    let waiting = Waiting::new(&w.cluster);
    let reports = Arc::new(AtomicUsize::new(0));
    let seen = reports.clone();
    waiting.on_damage(Arc::new(move |_| {
        seen.fetch_add(1, Ordering::SeqCst);
    }));
    std::fs::write(w.cluster.join(WAITING_FILE), b"{not json").unwrap();
    assert!(waiting.list().is_empty());
    assert!(waiting.gates().is_empty());
    assert!(waiting.list().is_empty());
    assert_eq!(reports.load(Ordering::SeqCst), 1);
}

#[test]
fn a_file_of_the_wrong_shape_reads_empty() {
    let w = world("waiting-shape", Seed::default());
    let waiting = Waiting::new(&w.cluster);
    for text in [
        r#"{"version": 2, "waiting": []}"#,
        r#"{"version": 1}"#,
        r#"{"version": 1, "waiting": [{"session": "a"}]}"#,
        "[]",
    ] {
        std::fs::write(w.cluster.join(WAITING_FILE), text).unwrap();
        assert!(waiting.list().is_empty(), "{text}");
    }
}

#[test]
fn adding_after_damage_sets_the_old_file_aside() {
    let w = world("waiting-aside", Seed::default());
    let waiting = Waiting::new(&w.cluster);
    std::fs::write(w.cluster.join(WAITING_FILE), b"garbage").unwrap();
    waiting.add(S1, &node("m1"), "m1").unwrap();
    assert_eq!(waiting.list().len(), 1);
    let kept = std::fs::read(w.cluster.join("waiting.json.damaged")).unwrap();
    assert_eq!(kept, b"garbage");
}

#[test]
fn adding_twice_replaces_and_removing_says_whether_it_was_there() {
    let w = world("waiting-twice", Seed::default());
    let waiting = Waiting::new(&w.cluster);
    waiting.add(S1, &node("m1"), "first").unwrap();
    waiting.add(S1, &node("m1"), "second\u{202e}").unwrap();
    let list = waiting.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "second");
    assert!(waiting.remove(S1).unwrap());
    assert!(!waiting.remove(S1).unwrap());
}

#[cfg(unix)]
#[test]
fn waiting_file_is_private_and_never_follows_a_link() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let w = world("waiting-private", Seed::default());
    let waiting = Waiting::new(&w.cluster);
    waiting.add(S1, &node("m1"), "m1").unwrap();
    let mode = std::fs::metadata(w.cluster.join(WAITING_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let other = w.cluster.join("elsewhere");
    std::fs::write(&other, b"keep").unwrap();
    std::fs::remove_file(w.cluster.join(WAITING_FILE)).unwrap();
    symlink(&other, w.cluster.join(WAITING_FILE)).unwrap();
    waiting.add(S1, &node("m1"), "m1").unwrap();
    assert_eq!(std::fs::read(&other).unwrap(), b"keep");
    let meta = std::fs::symlink_metadata(w.cluster.join(WAITING_FILE)).unwrap();
    assert!(meta.is_file());
    assert_eq!(waiting.list().len(), 1);
}
