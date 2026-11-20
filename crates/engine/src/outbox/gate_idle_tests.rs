use super::*;

fn facts(status: Option<&str>) -> Facts {
    Facts {
        now_ms: 10_000,
        job: Job::Live {
            status: status.map(String::from),
        },
        dialog_on_screen: false,
        queue_waiting: 0,
        safe: false,
        turn_over_since_ms: None,
    }
}

#[test]
fn a_live_idle_session_with_nothing_on_screen_is_idle_at_its_prompt() {
    assert!(idle_at_prompt(&facts(Some("idle"))));
}

#[test]
fn a_session_in_a_turn_or_with_no_status_is_not() {
    for status in [Some("busy"), Some("waiting"), Some("shell"), None] {
        assert!(!idle_at_prompt(&facts(status)), "{status:?}");
    }
}

#[test]
fn a_dialog_a_queue_or_safe_mode_is_not() {
    let mut dialog = facts(Some("idle"));
    dialog.dialog_on_screen = true;
    let mut queued = facts(Some("idle"));
    queued.queue_waiting = 1;
    let mut safe = facts(Some("idle"));
    safe.safe = true;
    for case in [dialog, queued, safe] {
        assert!(!idle_at_prompt(&case), "{case:?}");
    }
}

#[test]
fn a_session_that_is_gone_or_stopped_is_not() {
    let mut gone = facts(Some("idle"));
    gone.job = Job::Missing { since_ms: 1 };
    let mut stopped = facts(Some("idle"));
    stopped.job = Job::Stopped;
    assert!(!idle_at_prompt(&gone));
    assert!(!idle_at_prompt(&stopped));
}
