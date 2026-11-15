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
fn a_revoke_that_found_nothing_beats_a_deliver_still_waiting_for_its_job() {
    let rig = rig();
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[]));
    let uses = rig.uses.clone();
    let args = deliver_args(JOB, "one", "V", "NL-MARKER-1");
    let waiting = std::thread::spawn(move || uses.deliver(&from_vault(), &args));
    std::thread::sleep(Duration::from_millis(15));
    rig.uses
        .revoke(&from_vault(), &json!({"job": JOB}))
        .expect("revoked");
    *rig.fake.state.lock().unwrap() =
        Some(cluster("vault", &[job(JOB, "worker", JobState::Running)]));
    let err = waiting.join().unwrap().expect_err("dropped");
    assert!(!err.message.contains("NL-MARKER"));
    assert!(rig.store.get(JOB, "one", real_now()).unwrap().is_none());
    assert!(rig.marks.list().is_empty());
    assert_eq!(rig.masks.state(JOB), JobMasks::Never);
    deliver(&rig, JOB, "one", "NL-MARKER-1").expect("a later deliver is fine");
}

#[test]
fn a_revoke_of_other_names_does_not_stop_a_deliver() {
    let rig = rig();
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[]));
    let uses = rig.uses.clone();
    let args = deliver_args(JOB, "one", "V", "NL-MARKER-1");
    let waiting = std::thread::spawn(move || uses.deliver(&from_vault(), &args));
    std::thread::sleep(Duration::from_millis(15));
    rig.uses
        .revoke(&from_vault(), &json!({"job": JOB, "names": ["other"]}))
        .expect("revoked");
    *rig.fake.state.lock().unwrap() =
        Some(cluster("vault", &[job(JOB, "worker", JobState::Running)]));
    waiting.join().unwrap().expect("stored");
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

#[test]
fn a_node_whose_cluster_is_not_loaded_yet_refuses_unless_it_is_the_vault_node() {
    let on_disk = || -> Disk {
        let vault = id("vault");
        Arc::new(move || Some(vault.clone()))
    };
    let names = json!({"names": ["one"]});
    let rig = rig_full(Arc::new(real_now), "worker", on_disk());
    *rig.fake.state.lock().unwrap() = None;
    let got = rig
        .uses
        .use_secrets(&caller(700), &names)
        .expect("answered here");
    assert_eq!(got.expect_err("refused").message, NOT_LOADED);
    let rig = rig_full(Arc::new(real_now), "vault", on_disk());
    *rig.fake.state.lock().unwrap() = None;
    assert!(rig.uses.use_secrets(&caller(700), &names).is_none());
    let rig = rig_full(Arc::new(real_now), "worker", Arc::new(|| None));
    *rig.fake.state.lock().unwrap() = None;
    assert!(rig.uses.use_secrets(&caller(700), &names).is_none());
}
