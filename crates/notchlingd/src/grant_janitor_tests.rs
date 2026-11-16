use super::*;
use crate::cluster_redact::JobMasks;
use crate::grant_use::tests::{
    cluster, deliver_args, from_vault, job, put, real_now, rig, rig_running_both, Rig, JOB,
    OTHER_JOB,
};
use notchling_engine::cluster::Tool;
use std::sync::atomic::AtomicI64;

fn janitor_at(rig: &Rig, clock: Clock) -> Arc<GrantJanitor> {
    GrantJanitor::new(
        rig.store.clone(),
        (rig.masks.clone(), rig.marks.clone(), rig.roots.clone()),
        rig.fake.clone(),
        clock,
    )
}

fn janitor(rig: &Rig) -> Arc<GrantJanitor> {
    janitor_at(rig, Arc::new(real_now))
}

fn has(rig: &Rig, job: &str, name: &str) -> bool {
    rig.store.get(job, name, real_now()).unwrap().is_some()
}

#[test]
fn sweep_removes_expired_and_dead_jobs() {
    let rig = rig();
    put(&rig, JOB, "live", "V", "NL-MARKER-1");
    put(&rig, OTHER_JOB, "dead", "V", "NL-MARKER-2");
    put(&rig, "j-abcdefabcdef", "unknown", "V", "NL-MARKER-3");
    *rig.fake.state.lock().unwrap() = Some(cluster(
        "vault",
        &[
            job(JOB, "worker", JobState::Running),
            job(OTHER_JOB, "worker", JobState::Done),
        ],
    ));
    let swept = janitor(&rig).tick().expect("swept");
    assert_eq!(swept, 2, "the finished and the unknown job");
    assert!(has(&rig, JOB, "live"));
    assert!(!has(&rig, OTHER_JOB, "dead"));
    assert!(!has(&rig, "j-abcdefabcdef", "unknown"));
    let later = real_now() + 2 * 3_600_000;
    let expiring = janitor_at(&rig, Arc::new(move || later));
    assert_eq!(expiring.tick().expect("swept"), 1, "the expired grant");
    assert!(rig.store.get(JOB, "live", real_now()).unwrap().is_none());
}

#[test]
fn a_finished_job_is_ended_and_its_grants_go() {
    let rig = rig();
    rig.uses
        .deliver(&from_vault(), &deliver_args(JOB, "one", "V", "NL-MARKER-1"))
        .expect("stored");
    rig.roots.set(JOB, 500);
    let janitor = janitor(&rig);
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "one"), "a running job keeps its grants");
    assert!(!rig.masks.is_ended(JOB));
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Done)]));
    janitor.tick().expect("swept");
    assert!(!has(&rig, JOB, "one"), "job end deletes");
    assert!(rig.roots.is_ended(JOB));
    assert!(rig.masks.is_ended(JOB));
    assert_eq!(
        rig.masks.state(JOB),
        JobMasks::Masked,
        "masks outlive the job"
    );
    assert_eq!(rig.marks.list(), [JOB.to_string()], "the mark outlives it");
}

#[test]
fn a_pruned_job_record_keeps_its_masks_and_mark() {
    let rig = rig_running_both();
    rig.uses
        .deliver(&from_vault(), &deliver_args(JOB, "one", "V", "NL-MARKER-1"))
        .expect("stored");
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[]));
    janitor(&rig).tick().expect("swept");
    assert_eq!(rig.masks.state(JOB), JobMasks::Masked);
    assert_eq!(rig.marks.list(), [JOB.to_string()]);
    assert!(!has(&rig, JOB, "one"), "its grants still go");
}

#[test]
fn a_lost_job_missing_from_the_state_stays_lost_and_keyed() {
    let rig = rig();
    let one = keys(&["shell:j-0123456789ab"]);
    rig.marks.mark_with(JOB, &one).unwrap();
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[]));
    at(&rig, 1_000).tick().expect("swept");
    assert_eq!(rig.masks.state(JOB), JobMasks::Gone);
    assert_eq!(
        rig.masks.job_for_key("shell:j-0123456789ab"),
        Some(JOB.to_string())
    );
    assert_eq!(rig.marks.list(), [JOB.to_string()]);
}

#[test]
fn no_cluster_state_sweeps_grants_but_keeps_masks_and_marks() {
    let rig = rig();
    rig.uses
        .deliver(&from_vault(), &deliver_args(JOB, "one", "V", "NL-MARKER-1"))
        .expect("stored");
    *rig.fake.state.lock().unwrap() = None;
    janitor(&rig).tick().expect("swept");
    assert!(!has(&rig, JOB, "one"), "no grant without a cluster");
    assert_eq!(rig.masks.state(JOB), JobMasks::Masked);
    assert_eq!(rig.marks.list(), [JOB.to_string()]);
}

#[test]
fn a_root_for_a_job_not_yet_in_the_state_is_kept() {
    let rig = rig();
    let late = "j-dddddddddddd";
    rig.roots.set(late, 600);
    janitor(&rig).tick().expect("swept");
    assert!(!rig.roots.is_ended(late));
    assert_eq!(rig.roots.rooted(), [late.to_string()]);
}

const TTL: i64 = 12 * 3_600_000;

fn at(rig: &Rig, now: i64) -> Arc<GrantJanitor> {
    janitor_at(rig, Arc::new(move || now))
}

fn keys(list: &[&str]) -> Vec<String> {
    list.iter().map(|k| k.to_string()).collect()
}

fn finished(rig: &Rig) {
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Done)]));
}

#[test]
fn a_finished_jobs_mark_gets_one_end_time() {
    let rig = rig();
    rig.uses
        .deliver(&from_vault(), &deliver_args(JOB, "one", "V", "NL-MARKER-1"))
        .expect("stored");
    at(&rig, 1_000).tick().expect("swept");
    assert_eq!(rig.marks.marked()[0].ended_ms, None, "still running");
    finished(&rig);
    at(&rig, 5_000).tick().expect("swept");
    at(&rig, 9_000).tick().expect("swept");
    assert_eq!(rig.marks.marked()[0].ended_ms, Some(5_000));
}

#[test]
fn a_keyed_lost_jobs_masks_and_mark_outlast_the_twelve_hours() {
    let rig = rig();
    let one = keys(&["shell:j-0123456789ab"]);
    rig.marks.mark_with(JOB, &one).unwrap();
    rig.masks.mark_granted_as(JOB, &one);
    finished(&rig);
    let start = 1_000_000;
    at(&rig, start).tick().expect("swept");
    assert_eq!(rig.masks.state(JOB), JobMasks::Gone);
    at(&rig, start + 3 * TTL).tick().expect("swept");
    assert_eq!(rig.masks.state(JOB), JobMasks::Gone, "still refused");
    assert!(rig.masks.lost(JOB));
    assert_eq!(rig.marks.list(), [JOB.to_string()]);
    assert_eq!(
        rig.masks.job_for_key("shell:j-0123456789ab"),
        Some(JOB.to_string())
    );
}

#[test]
fn an_unkeyed_lost_job_stops_blocking_other_rows_after_the_twelve_hours_but_stays() {
    let rig = rig();
    rig.marks.mark_with(JOB, &[]).unwrap();
    rig.masks.mark_granted(JOB);
    finished(&rig);
    let start = 1_000_000;
    at(&rig, start).tick().expect("swept");
    at(&rig, start + TTL - 1).tick().expect("swept");
    assert!(rig.masks.unidentified_lost(), "not yet");
    at(&rig, start + TTL).tick().expect("swept");
    assert!(!rig.masks.unidentified_lost(), "other rows are served");
    assert_eq!(rig.masks.state(JOB), JobMasks::Gone, "its own masks stay");
    assert_eq!(rig.marks.list(), [JOB.to_string()], "and its mark");
}

#[test]
fn a_session_the_state_gains_is_picked_up_without_waiting_for_the_sweep() {
    let rig = rig();
    rig.marks.mark_with(JOB, &[]).unwrap();
    rig.masks.mark_granted(JOB);
    let mut claude = job(JOB, "worker", JobState::Running);
    claude.spec.tool = Tool::Claude;
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[claude.clone()]));
    let janitor = at(&rig, 1_000);
    janitor.tick().expect("swept");
    assert!(!janitor.changed(), "nothing new");
    claude.session = Some("c1".to_string());
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[claude]));
    assert!(janitor.changed(), "a session link is a change");
}

#[test]
fn a_job_with_live_masks_keeps_its_mark_past_the_twelve_hours() {
    let rig = rig();
    rig.uses
        .deliver(&from_vault(), &deliver_args(JOB, "one", "V", "NL-MARKER-1"))
        .expect("stored");
    finished(&rig);
    let start = real_now();
    at(&rig, start).tick().expect("swept");
    at(&rig, start + 2 * TTL).tick().expect("swept");
    assert_eq!(rig.masks.state(JOB), JobMasks::Masked);
    assert_eq!(rig.marks.list(), [JOB.to_string()]);
}

#[test]
fn a_lost_mark_of_a_running_job_never_ends() {
    let rig = rig();
    let one = keys(&["shell:j-0123456789ab"]);
    rig.marks.mark_with(JOB, &one).unwrap();
    rig.masks.mark_granted_as(JOB, &one);
    at(&rig, 1_000).tick().expect("swept");
    at(&rig, 1_000 + 3 * TTL).tick().expect("swept");
    assert_eq!(rig.masks.state(JOB), JobMasks::Gone);
    assert_eq!(rig.marks.list(), [JOB.to_string()]);
}

#[test]
fn the_janitor_fills_in_the_session_a_mark_lacked() {
    let rig = rig();
    rig.marks.mark_with(JOB, &[]).unwrap();
    rig.masks.mark_granted(JOB);
    assert!(rig.masks.unidentified_lost());
    let mut claude = job(JOB, "worker", JobState::Running);
    claude.spec.tool = Tool::Claude;
    claude.session = Some("c1".to_string());
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[claude]));
    at(&rig, 1_000).tick().expect("swept");
    assert_eq!(rig.marks.marked()[0].keys, keys(&["c1"]));
    assert_eq!(rig.masks.job_for_key("c1"), Some(JOB.to_string()));
    assert!(!rig.masks.unidentified_lost());
}

#[test]
fn the_thread_sweeps_on_a_state_change_and_stops() {
    let rig = rig_running_both();
    put(&rig, JOB, "one", "V", "NL-MARKER-1");
    let clock = Arc::new(AtomicI64::new(real_now()));
    let ticking = clock.clone();
    let janitor = janitor_at(&rig, Arc::new(move || ticking.load(Ordering::SeqCst)));
    janitor.spawn();
    std::thread::sleep(Duration::from_millis(300));
    assert!(has(&rig, JOB, "one"), "still live");
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Done)]));
    let until = Instant::now() + Duration::from_secs(10);
    while has(&rig, JOB, "one") && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!has(&rig, JOB, "one"), "swept after the state changed");
    janitor.close();
}
