use super::*;

fn event(name: &str, data: Value) -> Event {
    Event {
        event: name.to_owned(),
        data,
    }
}

fn shared() -> Shared {
    Shared {
        state: Mutex::new(State::default()),
        stop: AtomicBool::new(false),
        restart: Arc::default(),
    }
}

#[test]
fn a_hold_the_daemon_reports_is_taken_once_and_only_when_it_changed() {
    let s = shared();
    assert_eq!(s.take_hold(), None, "nothing said yet");
    s.apply(&event(topics::AWAKE_CHANGED, json!({"holding": "system"})));
    assert_eq!(s.take_hold(), Some(true));
    assert_eq!(s.take_hold(), None, "already taken");
    s.apply(&event(topics::AWAKE_CHANGED, json!({"holding": "display"})));
    assert_eq!(s.take_hold(), None, "still held, only the kind changed");
    s.apply(&event(topics::AWAKE_CHANGED, json!({"holding": null})));
    assert_eq!(s.take_hold(), Some(false));
}

#[test]
fn a_report_that_is_not_one_changes_nothing() {
    let s = shared();
    s.apply(&event(topics::AWAKE_CHANGED, json!({"holding": "system"})));
    s.take_hold();
    s.apply(&event(topics::AWAKE_CHANGED, json!({"holding": 3})));
    s.apply(&event(topics::AWAKE_CHANGED, json!({})));
    s.apply(&event(topics::AWAKE_CHANGED, json!("system")));
    assert_eq!(s.take_hold(), None);
}

#[test]
fn losing_the_daemon_empties_the_cup() {
    let s = shared();
    s.lost();
    assert_eq!(s.take_hold(), None, "there was nothing to empty");
    s.apply(&event(topics::AWAKE_CHANGED, json!({"holding": "system"})));
    s.take_hold();
    s.lost();
    assert_eq!(s.take_hold(), Some(false));
}

#[test]
fn a_hold_taken_and_dropped_before_anyone_looked_is_not_news() {
    let s = shared();
    s.apply(&event(topics::AWAKE_CHANGED, json!({"holding": "system"})));
    s.apply(&event(topics::AWAKE_CHANGED, json!({"holding": null})));
    assert_eq!(s.take_hold(), None);
}
