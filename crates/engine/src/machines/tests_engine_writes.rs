use super::registry::{write_lock, SdWorkerSettings};
use super::step::{CheckResult, Detected};
use super::tests_engine_world::*;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn the_facts_a_refresh_learned_wait_for_a_write_in_progress() {
    let mut s = Setup::new(|_, _| {
        vec![StepBuilder::new("a")
            .parse(|_, _| {
                CheckResult::pass("ok").detected(Detected {
                    home: Some("/Users/worker".into()),
                    ..Detected::default()
                })
            })
            .build()]
    });
    let mut m = mac("m1");
    m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
    s.machines = vec![m];
    let w = Arc::new(s.build());

    let (held_tx, held) = channel::<()>();
    let (release_tx, release) = channel::<()>();
    let writer = std::thread::spawn(move || {
        let _guard = write_lock();
        held_tx.send(()).unwrap();
        release.recv().unwrap();
    });
    held.recv().unwrap();
    let refresh = {
        let w = w.clone();
        std::thread::spawn(move || w.refresh("m1", false))
    };
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        lock(&w.store.saves).is_empty(),
        "the refresh's save waits for the write in progress"
    );
    release_tx.send(()).unwrap();
    writer.join().unwrap();
    refresh.join().unwrap();
    assert_eq!(lock(&w.store.saves).len(), 1, "then it saves");
}
