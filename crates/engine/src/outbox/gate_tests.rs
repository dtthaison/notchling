use super::*;

fn row(kind: Kind) -> Row {
    Row {
        n: 1,
        id: "id".into(),
        session: "s".into(),
        tool: Tool::Claude,
        kind,
        text: "hi".into(),
        options: None,
        via: Via::Chat,
        state: State::Queued,
        why: None,
        clear_box: false,
        queued_ms: 0,
        updated_ms: 0,
        attempt_ms: None,
        seen_before: None,
        queued_before: None,
        tries: 0,
        next_ms: None,
        noticed_ms: None,
        anchor: None,
        aside_at_ms: None,
        aside_ms: None,
        hold_ms: None,
        joined: 0,
        awaited: false,
    }
}

fn facts(now_ms: i64) -> Facts {
    Facts {
        now_ms,
        job: Job::Live {
            status: Some("busy".into()),
        },
        dialog_on_screen: false,
        queue_waiting: 0,
        safe: false,
        turn_over_since_ms: None,
    }
}

#[test]
fn missing_job_waits_then_is_gone_after_60s() {
    let mut f = facts(100_000);
    f.job = Job::Missing { since_ms: 40_001 };
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Retry,
            until_ms: 101_000
        }
    );
    f.job = Job::Missing { since_ms: 40_000 };
    assert_eq!(gate(&row(Kind::Prompt), &f), Gate::NeedsUser(State::Gone));
}

#[test]
fn stopped_job_needs_user() {
    let mut f = facts(1_000);
    f.job = Job::Stopped;
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::NeedsUser(State::Stopped)
    );
}

#[test]
fn an_unknown_job_waits_to_check_and_is_never_stopped() {
    let mut f = facts(10_000);
    f.job = Job::Unknown { since_ms: 10_000 };
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Checking,
            until_ms: 11_000
        }
    );
    f.now_ms = 10_000 + UNREADABLE_AFTER_MS - 1;
    assert!(matches!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Checking,
            ..
        }
    ));
}

#[test]
fn an_unknown_job_for_long_is_unreadable_and_keeps_waiting() {
    let mut f = facts(10_000 + UNREADABLE_AFTER_MS);
    f.job = Job::Unknown { since_ms: 10_000 };
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Unreadable,
            until_ms: 10_000 + UNREADABLE_AFTER_MS + 1_000
        }
    );
    f.now_ms += 3_600_000;
    assert!(matches!(
        gate(&row(Kind::Command), &f),
        Gate::Wait {
            why: Why::Unreadable,
            ..
        }
    ));
}

#[test]
fn a_done_job_is_tried_and_is_stopped_only_after_the_tries_fail() {
    let mut f = facts(1_000);
    f.job = Job::Done;
    let mut r = row(Kind::Prompt);
    assert_eq!(gate(&r, &f), Gate::Go { guarded: false });
    r.tries = DONE_STOPS_AFTER_TRIES - 1;
    assert_eq!(gate(&r, &f), Gate::Go { guarded: false });
    r.tries = DONE_STOPS_AFTER_TRIES;
    assert_eq!(gate(&r, &f), Gate::NeedsUser(State::Stopped));
}

#[test]
fn waiting_status_waits_for_dialog() {
    let mut f = facts(1_000);
    f.job = Job::Live {
        status: Some("waiting".into()),
    };
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Dialog,
            until_ms: 1_000 + DIALOG_RECHECK_MS
        }
    );
}

#[test]
fn dialog_on_screen_waits() {
    let mut f = facts(1_000);
    f.dialog_on_screen = true;
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Dialog,
            until_ms: 1_000 + DIALOG_RECHECK_MS
        }
    );
}

#[test]
fn retry_waits_until_next_ms() {
    let mut r = row(Kind::Prompt);
    r.next_ms = Some(31_000);
    assert_eq!(
        gate(&r, &facts(1_000)),
        Gate::Wait {
            why: Why::Retry,
            until_ms: 31_000
        }
    );
    assert_eq!(gate(&r, &facts(31_000)), Gate::Go { guarded: false });
}

#[test]
fn command_waits_for_turn_end() {
    assert_eq!(
        gate(&row(Kind::Command), &facts(1_000)),
        Gate::Wait {
            why: Why::Turn,
            until_ms: 2_000
        }
    );
}

#[test]
fn safe_mode_waits_for_turn_end() {
    let mut f = facts(1_000);
    f.safe = true;
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Turn,
            until_ms: 2_000
        }
    );
}

#[test]
fn turn_over_for_3s_goes_guarded() {
    let mut f = facts(10_000);
    f.safe = true;
    f.turn_over_since_ms = Some(10_000 - (TURN_SETTLE_MS - 1));
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Turn,
            until_ms: 11_000
        }
    );
    f.turn_over_since_ms = Some(10_000 - TURN_SETTLE_MS);
    assert_eq!(gate(&row(Kind::Prompt), &f), Gate::Go { guarded: true });
}

#[test]
fn queue_entry_waits() {
    let mut f = facts(1_000);
    f.queue_waiting = 1;
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Queue,
            until_ms: 2_000
        }
    );
}

fn idle_with_queue(now_ms: i64, over_for_ms: Option<i64>, status: &str) -> Facts {
    let mut f = facts(now_ms);
    f.job = Job::Live {
        status: Some(status.into()),
    };
    f.queue_waiting = 1;
    f.turn_over_since_ms = over_for_ms.map(|ms| now_ms - ms);
    f
}

#[test]
fn stale_queue_entries_on_an_idle_job_go_guarded() {
    for status in ["idle", "shell"] {
        assert_eq!(
            gate(
                &row(Kind::Prompt),
                &idle_with_queue(50_000, Some(QUEUE_STALE_MS), status)
            ),
            Gate::Go { guarded: true }
        );
        assert_eq!(
            gate(
                &row(Kind::Prompt),
                &idle_with_queue(50_000, Some(QUEUE_STALE_MS * 6), status)
            ),
            Gate::Go { guarded: true }
        );
    }
}

#[test]
fn queue_entries_while_working_still_wait() {
    let wait = Gate::Wait {
        why: Why::Queue,
        until_ms: 51_000,
    };
    for status in [Some("busy"), None] {
        let mut f = idle_with_queue(50_000, Some(60_000), "busy");
        f.job = Job::Live {
            status: status.map(String::from),
        };
        assert_eq!(gate(&row(Kind::Prompt), &f), wait);
    }
}

#[test]
fn queue_entries_right_after_a_turn_still_wait() {
    let wait = Gate::Wait {
        why: Why::Queue,
        until_ms: 51_000,
    };
    assert_eq!(
        gate(
            &row(Kind::Prompt),
            &idle_with_queue(50_000, Some(QUEUE_STALE_MS - 1), "idle")
        ),
        wait
    );
    assert_eq!(
        gate(&row(Kind::Prompt), &idle_with_queue(50_000, None, "idle")),
        wait
    );
}

#[test]
fn idle_or_busy_session_goes_at_once() {
    assert_eq!(
        gate(&row(Kind::Prompt), &facts(1_000)),
        Gate::Go { guarded: false }
    );
    let mut f = facts(1_000);
    f.job = Job::Live { status: None };
    assert_eq!(gate(&row(Kind::Prompt), &f), Gate::Go { guarded: false });
}

#[test]
fn retry_back_off_beats_command_and_queue() {
    let mut r = row(Kind::Command);
    r.next_ms = Some(31_000);
    let mut f = facts(1_000);
    f.queue_waiting = 2;
    f.safe = true;
    let expected = Gate::Wait {
        why: Why::Retry,
        until_ms: 31_000,
    };
    assert_eq!(gate(&r, &f), expected);
    r.kind = Kind::Prompt;
    assert_eq!(gate(&r, &f), expected);
}
