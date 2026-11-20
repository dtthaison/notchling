use super::state::{with_session, Session, MAX_AGENTS, MAX_RESERVED, TTL};
use crate::sessions::testutil::TempDir;

const NOW: f64 = 1_800_000_000.0;

#[test]
fn the_sixth_builder_is_over_a_cap_of_five() {
    let mut held = Session::default();
    for i in 0..5 {
        assert!(!held.reserve(&format!("t{i}"), NOW, 5), "builder {i}");
    }
    assert!(held.reserve("t5", NOW, 5));
    assert!(held.reserve("t6", NOW, 5));
}

#[test]
fn the_same_call_seen_twice_counts_once() {
    let mut held = Session::default();
    assert!(!held.reserve("t0", NOW, 1));
    assert!(!held.reserve("t0", NOW + 1.0, 1));
    assert_eq!(held.working(), 1);
    assert!(held.reserve("t1", NOW + 2.0, 1));
}

#[test]
fn a_reservation_expires_when_nothing_releases_it() {
    let mut held = Session::default();
    for i in 0..5 {
        held.reserve(&format!("t{i}"), NOW, 5);
    }
    assert!(held.reserve("late", NOW + TTL - 1.0, 5));
    assert!(!held.reserve("later", NOW + TTL + 1.0, 5));
    assert_eq!(held.working(), 2);
}

#[test]
fn a_helper_that_keeps_working_keeps_its_reservation() {
    let mut held = Session::default();
    held.reserve("t0", NOW, 5);
    held.claim("agent-a", NOW);
    held.count("agent-a", NOW + TTL - 10.0);
    held.reserve("t1", NOW + TTL + 5.0, 5);
    assert_eq!(held.working(), 2);
}

#[test]
fn a_stop_releases_the_helper_it_started() {
    let mut held = Session::default();
    held.reserve("t0", NOW, 5);
    held.reserve("t1", NOW, 5);
    held.claim("agent-a", NOW);
    held.claim("agent-b", NOW);
    assert_eq!(held.working(), 2);
    held.release("agent-a");
    assert_eq!(held.working(), 1);
    held.release("agent-unknown");
    assert_eq!(held.working(), 1);
    held.release("agent-b");
    assert_eq!(held.working(), 0);
}

#[test]
fn a_start_claims_each_reservation_once() {
    let mut held = Session::default();
    held.reserve("t0", NOW, 5);
    held.reserve("t1", NOW, 5);
    held.claim("agent-a", NOW);
    held.claim("agent-a", NOW);
    held.claim("agent-b", NOW);
    held.claim("agent-c", NOW);
    let owners: Vec<_> = held.reserved.iter().map(|r| r.agent.clone()).collect();
    assert_eq!(
        owners,
        vec![Some("agent-a".to_string()), Some("agent-b".to_string())]
    );
}

#[test]
fn the_file_stays_small() {
    let mut held = Session::default();
    for i in 0..(MAX_RESERVED + 20) {
        held.reserve(&format!("t{i}"), NOW, 5);
    }
    assert_eq!(held.working(), MAX_RESERVED);
    for i in 0..(MAX_AGENTS + 20) {
        held.count(&format!("a{i}"), NOW);
    }
    assert_eq!(held.calls.len(), MAX_AGENTS);
}

#[test]
fn calls_are_counted_per_helper_and_forgotten_on_stop() {
    let mut held = Session::default();
    assert_eq!(held.count("a", NOW), 1);
    assert_eq!(held.count("a", NOW), 2);
    assert_eq!(held.count("b", NOW), 1);
    held.release("a");
    assert_eq!(held.count("a", NOW), 1);
}

#[test]
fn a_session_survives_between_calls() {
    let dir = TempDir::new("guard-state");
    assert_eq!(
        with_session(dir.root(), "s1", |s| s.reserve("t0", NOW, 5)),
        Some(false)
    );
    assert_eq!(with_session(dir.root(), "s1", |s| s.working()), Some(1));
    assert_eq!(with_session(dir.root(), "s2", |s| s.working()), Some(0));
    let text = std::fs::read_to_string(dir.path("sessions/s1.json")).expect("file");
    assert!(text.contains("t0"));
}

#[test]
fn a_damaged_or_huge_file_starts_over() {
    let dir = TempDir::new("guard-state");
    dir.write("sessions/s1.json", "{not json");
    assert_eq!(with_session(dir.root(), "s1", |s| s.working()), Some(0));
    dir.write("sessions/s2.json", &"x".repeat(200_000));
    assert_eq!(with_session(dir.root(), "s2", |s| s.working()), Some(0));
}

#[test]
fn a_held_lock_gives_up_instead_of_waiting() {
    let dir = TempDir::new("guard-state");
    let started = std::time::Instant::now();
    let inner = with_session(dir.root(), "s1", |_| {
        with_session(dir.root(), "s1", |s| s.working())
    });
    assert_eq!(inner, Some(None));
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn old_session_files_are_swept_when_a_new_session_starts() {
    let dir = TempDir::new("guard-state");
    let old = dir.write("sessions/old.json", "{}");
    let file = std::fs::File::options()
        .write(true)
        .open(&old)
        .expect("open");
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(5 * 86_400);
    file.set_modified(long_ago).expect("time");
    drop(file);
    with_session(dir.root(), "fresh", |s| s.working());
    assert!(!old.exists());
}
