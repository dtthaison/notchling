//! The window's edits against the engine's own writes, and the work it keeps off the UI thread.

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notchling_engine::machines::registry::{write_lock, AdminAccess};

use super::model_tests::{lock, mac, scripted_world};

#[test]
fn an_edit_waits_for_a_save_of_learned_facts_in_progress() {
    let mut w = scripted_world(vec![mac("m1")]);
    // a refresh is saving what it learned (the engine holds the same lock for that)
    let released = Arc::new(Mutex::new(false));
    let (held_tx, held) = channel::<()>();
    let flag = released.clone();
    let saver = std::thread::spawn(move || {
        let _guard = write_lock();
        held_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        *lock(&flag) = true;
    });
    held.recv().unwrap();
    w.model.set_admin("m1", AdminAccess::RootSsh);
    assert!(
        *lock(&released),
        "the edit's read-modify-write ran after the other save, not across it"
    );
    saver.join().unwrap();
    assert_eq!(w.model.machine("m1").unwrap().admin(), AdminAccess::RootSsh);
    assert_eq!(*lock(&w.registry.saves), 1);
}
