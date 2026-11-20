use super::state::{with_session, Session, MAX_AGENTS, MAX_RESERVED, PENDING_TTL, TTL};
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
        held.claim(&format!("a{i}"), NOW, None);
    }
    assert!(held.reserve("late", NOW + TTL - 1.0, 5));
    assert!(!held.reserve("later", NOW + TTL + 1.0, 5));
    assert_eq!(held.working(), 2);
}

#[test]
fn a_helper_that_keeps_working_keeps_its_reservation() {
    let mut held = Session::default();
    held.reserve("t0", NOW, 5);
    held.claim("agent-a", NOW, None);
    held.count("agent-a", NOW + TTL - 10.0);
    held.reserve("t1", NOW + TTL + 5.0, 5);
    assert_eq!(held.working(), 2);
}

#[test]
fn a_stop_releases_the_helper_it_started() {
    let mut held = Session::default();
    held.reserve("t0", NOW, 5);
    held.reserve("t1", NOW, 5);
    held.claim("agent-a", NOW, None);
    held.claim("agent-b", NOW, None);
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
    held.claim("agent-a", NOW, None);
    held.claim("agent-a", NOW, None);
    held.claim("agent-b", NOW, None);
    held.claim("agent-c", NOW, None);
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

#[test]
fn a_spawn_that_never_started_stops_counting_soon() {
    let mut held = Session::default();
    for i in 0..5 {
        held.reserve(&format!("t{i}"), NOW, 5);
    }
    assert!(held.reserve("t5", NOW + PENDING_TTL - 1.0, 5));
    assert!(!held.reserve("t6", NOW + PENDING_TTL + 1.0, 5));
    assert_eq!(held.working(), 2);
}

#[test]
fn a_start_with_a_call_id_takes_that_reservation_not_the_oldest() {
    let mut held = Session::default();
    held.reserve("denied", NOW, 5);
    held.reserve("real", NOW + 1.0, 5);
    held.claim("agent-a", NOW + 2.0, Some("real"));
    let owners: Vec<_> = held
        .reserved
        .iter()
        .map(|r| (r.id.as_str(), r.agent.as_deref()))
        .collect();
    assert_eq!(owners, vec![("denied", None), ("real", Some("agent-a"))]);
    held.release("agent-a");
    assert_eq!(held.working(), 1);
}

#[test]
fn a_start_with_an_id_that_is_claimed_or_unknown_falls_back_to_the_oldest_open() {
    let mut held = Session::default();
    held.reserve("t0", NOW, 5);
    held.reserve("t1", NOW, 5);
    held.claim("agent-a", NOW, Some("unknown"));
    assert_eq!(held.reserved[0].agent.as_deref(), Some("agent-a"));
    held.claim("agent-b", NOW, Some("t0"));
    assert_eq!(held.reserved[1].agent, None);
}

#[test]
fn a_full_call_table_forgets_the_helper_idle_longest() {
    let mut held = Session::default();
    for i in 0..MAX_AGENTS {
        held.count(&format!("a{i:03}"), NOW + i as f64);
    }
    held.count("a000", NOW + 1_000.0);
    held.count("fresh", NOW + 1_001.0);
    assert_eq!(held.calls.len(), MAX_AGENTS);
    assert!(held.calls.contains_key("a000"));
    assert!(!held.calls.contains_key("a001"));
    assert!(held.calls.contains_key("fresh"));
}

fn age(path: &std::path::Path, days: u64) {
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open");
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
    file.set_modified(when).expect("time");
}

#[test]
fn a_sweep_takes_old_files_with_their_lock_and_leaves_used_ones() {
    let dir = TempDir::new("guard-state");
    let old_data = dir.write("sessions/old.json", "{}");
    let old_lock = dir.write("sessions/old.lock", "");
    let busy_data = dir.write("sessions/busy.json", "{}");
    let busy_lock = dir.write("sessions/busy.lock", "");
    let recent_lock = dir.write("sessions/recent.lock", "");
    for path in [&old_data, &old_lock, &busy_data, &busy_lock] {
        age(path, 5);
    }
    let held = std::fs::OpenOptions::new()
        .write(true)
        .open(&busy_lock)
        .expect("open");
    assert_eq!(
        unsafe {
            libc::flock(
                std::os::unix::io::AsRawFd::as_raw_fd(&held),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        },
        0
    );
    with_session(dir.root(), "fresh", |s| s.working());
    assert!(!old_data.exists() && !old_lock.exists());
    assert!(busy_data.exists() && busy_lock.exists());
    assert!(recent_lock.exists());
    drop(held);
}

#[test]
fn a_session_whose_lock_was_swept_while_waiting_still_gets_exclusive_use() {
    let dir = TempDir::new("guard-state");
    with_session(dir.root(), "s1", |s| s.reserve("t0", NOW, 5));
    let lock = dir.path("sessions/s1.lock");
    std::fs::remove_file(&lock).expect("remove");
    assert_eq!(with_session(dir.root(), "s1", |s| s.working()), Some(1));
    assert!(lock.exists());
}

#[test]
fn a_session_file_in_the_old_shape_starts_that_session_fresh() {
    let dir = TempDir::new("guard-state");
    dir.write(
        "sessions/s1.json",
        r#"{"reserved":[{"id":"t0","ts":1.0,"agent":null}],"calls":{"a":3}}"#,
    );
    assert_eq!(with_session(dir.root(), "s1", |s| s.working()), Some(0));
    assert_eq!(
        with_session(dir.root(), "s1", |s| s.count("a", NOW)),
        Some(1)
    );
}

#[test]
fn a_session_that_woke_up_is_not_swept_from_under_itself() {
    let dir = TempDir::new("guard-state");
    let data = dir.write("sessions/old.json", "{}");
    let lock = dir.write("sessions/old.lock", "");
    age(&data, 5);
    age(&lock, 5);
    let fresh = std::fs::File::options()
        .write(true)
        .open(&data)
        .expect("open");
    fresh
        .set_modified(std::time::SystemTime::now())
        .expect("time");
    drop(fresh);
    with_session(dir.root(), "other", |s| s.working());
    assert!(data.exists() && lock.exists());
}

#[test]
fn an_old_file_without_a_lock_is_swept_under_a_lock_and_leaves_none_behind() {
    let dir = TempDir::new("guard-state");
    let data = dir.write("sessions/gone.json", "{}");
    let tmp = dir.write("sessions/gone.json.tmp", "{");
    age(&data, 5);
    age(&tmp, 5);
    with_session(dir.root(), "other", |s| s.working());
    assert!(!data.exists() && !tmp.exists());
    assert!(!dir.path("sessions/gone.lock").exists());
}

#[test]
fn a_file_with_a_lock_somebody_holds_is_left_for_later() {
    let dir = TempDir::new("guard-state");
    let data = dir.write("sessions/held.json", "{}");
    age(&data, 5);
    let lock = dir.write("sessions/held.lock", "");
    let held = std::fs::OpenOptions::new()
        .write(true)
        .open(&lock)
        .expect("open");
    assert_eq!(
        unsafe {
            libc::flock(
                std::os::unix::io::AsRawFd::as_raw_fd(&held),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        },
        0
    );
    with_session(dir.root(), "other", |s| s.working());
    assert!(data.exists() && lock.exists());
    drop(held);
}
