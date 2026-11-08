//! What happens to a session's permission when it ends, and how many `agy` processes are open.

use super::*;

#[test]
fn closing_a_session_resolves_the_permission_waiting_in_it() {
    let rig = rig();
    let process = loaded(&rig);
    ask(&process, "a");
    rig.events
        .wait("pending", |c| matches!(c, Change::PermissionPending(_)));
    rig.runner.close(SESSION);
    rig.events.wait("Closed", |c| *c == Change::Closed);
    let events = rig.events.all();
    assert!(
        index_of(&events, cancelled_a) < index_of(&events, |c| *c == Change::Closed),
        "{events:?}"
    );
    assert_eq!(rig.events.count(cancelled_a), 1);
}

#[test]
fn a_process_that_dies_with_a_permission_waiting_resolves_it() {
    let rig = rig();
    let process = loaded(&rig);
    ask(&process, "a");
    rig.events
        .wait("pending", |c| matches!(c, Change::PermissionPending(_)));
    process.die(Some("agy crashed"));
    rig.events.wait("Closed", |c| *c == Change::Closed);
    let events = rig.events.all();
    assert!(
        index_of(&events, cancelled_a) < index_of(&events, |c| *c == Change::Closed),
        "{events:?}"
    );
    assert_eq!(rig.events.count(cancelled_a), 1);
}

#[test]
fn only_so_many_processes_are_open_at_once() {
    let rig = rig();
    let r = &rig.runner;
    r.set_max_open(2);
    r.open("a", "/work", false).unwrap();
    r.open("b", "/work", false).unwrap();
    let refused = r.open("c", "/work", false).unwrap_err();
    assert!(
        matches!(&refused, RunError::Refused(why) if why.contains('2')),
        "{refused:?}"
    );
    assert_eq!(rig.spawner.spawned().len(), 2, "nothing was started for it");
    // A new session needs a process too.
    assert!(matches!(
        r.start("/work", Duration::from_secs(1)),
        Err(RunError::Refused(_))
    ));
    assert_eq!(rig.spawner.spawned().len(), 2);
    // An open session is not a new process.
    r.open("a", "/work", false).unwrap();
    // Closing one makes room.
    r.close("a");
    r.open("c", "/work", false).unwrap();
    assert_eq!(rig.spawner.spawned().len(), 3);
}

#[test]
fn closing_everything_leaves_no_process() {
    let rig = rig();
    let process = loaded(&rig);
    rig.runner.close_all();
    assert!(process.killed());
    assert!(!rig.runner.is_open(SESSION));
}
