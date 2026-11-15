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
