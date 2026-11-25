use crate::presence::{Presence, DROP_AFTER};
use notchling_events::kinds::security::DetachCause;
use std::path::Path;
use std::time::{Duration, Instant};

const EXE: &str = "/Applications/Notchling.app/Contents/MacOS/notchling-app";

#[test]
fn first_connection_attaches_once() {
    let presence = Presence::new();
    let now = Instant::now();
    let attached = presence
        .on_hello(41, "0.1.0", Some(Path::new(EXE)), now)
        .expect("attaches");
    assert_eq!(attached.pid.0, 41);
    assert_eq!(attached.exe.as_str(), "notchling-app");
    assert_eq!(attached.version.as_str(), "0.1.0");
    assert!(presence.on_hello(41, "0.1.0", None, now).is_none());
    assert!(presence.on_hello(42, "0.1.0", None, now).is_some());
}

#[test]
fn many_connections_one_attach() {
    let presence = Presence::new();
    let now = Instant::now();
    let mut attaches = 0;
    for _ in 0..5 {
        attaches += usize::from(presence.on_hello(7, "0.1.0", None, now).is_some());
    }
    assert_eq!(attaches, 1);
    for _ in 0..5 {
        presence.on_close(7, now);
    }
    assert!(presence
        .due(now + DROP_AFTER - Duration::from_secs(1))
        .is_empty());
}

#[test]
fn close_then_reopen_within_ten_seconds_stays_attached() {
    let presence = Presence::new();
    let start = Instant::now();
    presence.on_hello(7, "0.1.0", None, start);
    presence.on_close(7, start + Duration::from_secs(1));
    assert!(presence
        .on_hello(7, "0.1.0", None, start + Duration::from_secs(5))
        .is_none());
    assert!(presence.due(start + Duration::from_secs(60)).is_empty());
    presence.on_close(7, start + Duration::from_secs(61));
    assert!(presence.due(start + Duration::from_secs(65)).is_empty());
    assert_eq!(presence.due(start + Duration::from_secs(72)).len(), 1);
}

#[test]
fn close_for_ten_seconds_detaches_dropped() {
    let presence = Presence::new();
    let start = Instant::now();
    presence.on_hello(7, "0.1.0", None, start);
    presence.on_close(7, start + Duration::from_secs(3));
    assert!(presence.due(start + Duration::from_secs(12)).is_empty());
    let gone = presence.due(start + Duration::from_secs(13));
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].cause.name(), DetachCause::Dropped.name());
    assert_eq!(gone[0].attached_ms.0, 3_000);
    assert!(presence.due(start + Duration::from_secs(40)).is_empty());
    assert!(presence
        .on_hello(7, "0.1.0", None, start + Duration::from_secs(41))
        .is_some());
}
