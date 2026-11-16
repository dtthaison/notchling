use super::*;
use crate::cluster_redact::{mask_tail, JobMasks};
use crate::grant_janitor::GrantJanitor;
use crate::grant_wiring::GrantParts;
use std::sync::atomic::{AtomicBool, Ordering};

fn deliver(rig: &Rig, job: &str, name: &str, value: &str) -> Result<Value, ApiError> {
    rig.uses
        .deliver(&from_vault(), &deliver_args(job, name, "V", value))
}

fn janitor(rig: &Rig) -> Arc<GrantJanitor> {
    GrantJanitor::new(
        rig.store.clone(),
        (rig.masks.clone(), rig.marks.clone(), rig.roots.clone()),
        rig.fake.clone(),
        Arc::new(real_now),
    )
}

#[test]
fn a_restored_job_stays_unservable_after_a_later_grant() {
    let rig = rig();
    deliver(&rig, JOB, "one", "NL-MARKER-1").expect("stored");
    let restarted = GrantParts::new(rig.dir.path());
    let two = vec![(
        "two".to_string(),
        GrantValue::new("NL-MARKER-2".to_string()),
    )];
    restarted.masks.add(JOB, two).expect("added");
    assert!(restarted.masks.lost(JOB));
    let err =
        mask_tail(&restarted.masks, JOB, 0, &[], b"old NL-MARKER-1", true).expect_err("not served");
    assert!(err.contains(JOB));
    assert!(!err.contains("NL-MARKER"));
    assert_eq!(restarted.masks.state(OTHER_JOB), JobMasks::Never);
    assert!(!restarted.masks.lost(OTHER_JOB));
}

#[test]
fn a_revoke_that_found_nothing_beats_a_deliver_that_arrived_before_it() {
    let rig = rig();
    let arrived = rig.uses.arrived();
    rig.uses
        .revoke(&from_vault(), &json!({"job": JOB}))
        .expect("revoked");
    let args = deliver_args(JOB, "one", "V", "NL-MARKER-1");
    let err = rig
        .uses
        .deliver_at(&from_vault(), &args, arrived)
        .expect_err("dropped");
    assert!(!err.message.contains("NL-MARKER"));
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_none());
    assert!(rig.marks.list().is_empty());
    assert_eq!(rig.masks.state(JOB), JobMasks::Never);
    assert!(!rig.roots.is_pending(JOB, real_now()));
    deliver(&rig, JOB, "one", "NL-MARKER-1").expect("a later deliver is fine");
}

#[test]
fn a_revoke_of_other_names_does_not_stop_a_deliver() {
    let rig = rig();
    let arrived = rig.uses.arrived();
    rig.uses
        .revoke(&from_vault(), &json!({"job": JOB, "names": ["other"]}))
        .expect("revoked");
    let args = deliver_args(JOB, "one", "V", "NL-MARKER-1");
    rig.uses
        .deliver_at(&from_vault(), &args, arrived)
        .expect("stored");
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_some());
}

#[test]
fn a_deliver_and_a_sweep_both_wait_for_the_gate() {
    let rig = rig();
    let guard = rig.roots.gate();
    let done = Arc::new(AtomicBool::new(false));
    let (uses, flag) = (rig.uses.clone(), done.clone());
    let args = deliver_args(JOB, "one", "V", "NL-MARKER-1");
    let delivering = std::thread::spawn(move || {
        let out = uses.deliver(&from_vault(), &args);
        flag.store(true, Ordering::SeqCst);
        out
    });
    let swept = Arc::new(AtomicBool::new(false));
    let (sweeping, seen) = (janitor(&rig), swept.clone());
    let sweep = std::thread::spawn(move || {
        let out = sweeping.tick();
        seen.store(true, Ordering::SeqCst);
        out
    });
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !done.load(Ordering::SeqCst),
        "the deliver waits for the gate"
    );
    assert!(
        !swept.load(Ordering::SeqCst),
        "the sweep waits for the gate"
    );
    drop(guard);
    delivering.join().unwrap().expect("stored");
    sweep.join().unwrap().expect("swept");
}

#[test]
fn a_grant_of_a_running_job_survives_a_sweep() {
    let rig = rig();
    deliver(&rig, JOB, "one", "NL-MARKER-1").expect("stored");
    janitor(&rig).tick().expect("swept");
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_some());
}

fn unloaded(me: Option<&str>, disk: OnDisk) -> Option<Result<Value, ApiError>> {
    let rig = rig_full(Arc::new(real_now), "worker", Arc::new(move || disk.clone()));
    *rig.fake.state.lock().unwrap() = None;
    *rig.fake.me.lock().unwrap() = me.map(id);
    rig.uses
        .use_secrets(&caller(700), &json!({"names": ["one"]}))
}

fn cluster_on_disk(own: Option<&str>) -> OnDisk {
    OnDisk::Cluster {
        vault: id("vault"),
        me: own.map(id),
    }
}

#[test]
fn a_node_whose_cluster_is_not_loaded_yet_refuses_unless_it_is_the_vault_node() {
    let got = unloaded(Some("worker"), cluster_on_disk(Some("worker"))).expect("answered here");
    assert_eq!(got.expect_err("refused").message, NOT_LOADED);
    assert!(unloaded(Some("vault"), cluster_on_disk(Some("vault"))).is_none());
    assert!(unloaded(Some("worker"), OnDisk::Nothing).is_none());
}

#[test]
fn the_persisted_own_id_decides_when_the_node_is_not_up_yet() {
    assert!(
        unloaded(None, cluster_on_disk(Some("vault"))).is_none(),
        "the vault node's own persisted id answers locally"
    );
    let got = unloaded(None, cluster_on_disk(Some("worker"))).expect("answered here");
    assert_eq!(got.expect_err("refused").message, NOT_LOADED);
    let got = unloaded(None, cluster_on_disk(None)).expect("answered here");
    assert_eq!(
        got.expect_err("refused").message,
        NOT_LOADED,
        "an unknown own id can't claim to be the vault"
    );
}

#[test]
fn a_persisted_cluster_that_cannot_be_read_is_not_taken_for_none() {
    let got = unloaded(Some("vault"), OnDisk::Unreadable).expect("answered here");
    assert_eq!(got.expect_err("refused").message, NOT_LOADED);
    let got = unloaded(None, OnDisk::Unreadable).expect("answered here");
    assert_eq!(got.expect_err("refused").message, NOT_LOADED);
}

#[test]
fn a_revoke_handled_after_a_deliver_arrived_beats_it() {
    let rig = rig();
    let arrived = rig.uses.arrived();
    rig.uses
        .revoke(&from_vault(), &json!({"job": JOB}))
        .expect("revoked");
    let args = deliver_args(JOB, "one", "V", "NL-MARKER-1");
    let err = rig
        .uses
        .deliver_at(&from_vault(), &args, arrived)
        .expect_err("overtaken");
    assert!(!err.message.contains("NL-MARKER"));
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_none());
    assert!(rig.marks.list().is_empty());
    deliver(&rig, JOB, "one", "NL-MARKER-1").expect("a deliver arriving later is fine");
}

#[test]
fn the_dispatcher_stamps_a_deliver_when_it_arrives() {
    let rig = rig();
    let calls = UseCalls::new(
        Arc::new(crate::cluster_testkit::FakeCalls::default()),
        rig.uses.clone(),
    );
    let args = deliver_args(JOB, "one", "V", "NL-MARKER-1");
    calls
        .call(crate::grant_broker::GRANT_DELIVER, &args, &from_vault())
        .expect("stored");
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_some());
}
