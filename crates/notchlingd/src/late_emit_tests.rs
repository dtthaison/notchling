use super::LateEmit;
use notchling_events::kinds::security::{DaemonStopped, StopCause};
use notchling_events::spec::Outcome;
use notchling_events::{Emit, Event, MemorySink, Millis};
use std::sync::Arc;

fn stopped() -> Event {
    Event::new(
        &DaemonStopped {
            cause: StopCause::Quit,
            uptime_ms: Millis(5),
        },
        Outcome::Info,
        1,
    )
}

#[test]
fn late_emit_forwards_after_set_and_counts_before() {
    let late = LateEmit::new();
    late.push(stopped());
    assert!(late.commit(stopped()).is_err());
    assert_eq!(late.lost(), 2);
    let sink = Arc::new(MemorySink::new());
    late.set(sink.clone());
    late.push(stopped());
    assert!(late.commit(stopped()).is_ok());
    assert_eq!(sink.names(), vec!["daemon.stopped", "daemon.stopped"]);
    assert_eq!(late.lost(), 2);
}

#[test]
fn a_second_set_is_ignored() {
    let late = LateEmit::new();
    let first = Arc::new(MemorySink::new());
    let second = Arc::new(MemorySink::new());
    late.set(first.clone());
    late.set(second.clone());
    late.push(stopped());
    assert_eq!(first.events().len(), 1);
    assert!(second.events().is_empty());
}
