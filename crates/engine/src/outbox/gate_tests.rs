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
        done_from: None,
        unsent: false,
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

fn checking_since(kind: Kind, since_ms: i64) -> Row {
    Row {
        state: State::Waiting,
        why: None,
        updated_ms: since_ms,
        ..row(kind)
    }
}

fn unknown(now_ms: i64) -> Facts {
    Facts {
        job: Job::Unknown,
        ..facts(now_ms)
    }
}

#[test]
fn an_unknown_job_waits_to_check_and_is_never_stopped() {
    let mut f = unknown(10_000);
    assert_eq!(
        gate(&row(Kind::Prompt), &f),
        Gate::Wait {
            why: Why::Checking,
            until_ms: 11_000
        }
    );
    f.now_ms = 10_000 + UNREADABLE_AFTER_MS - 1;
    assert!(matches!(
        gate(&checking_since(Kind::Prompt, 10_000), &f),
        Gate::Wait {
            why: Why::Checking,
            ..
        }
    ));
}

#[test]
fn an_unknown_job_for_long_is_unreadable_and_keeps_waiting() {
    let mut f = unknown(10_000 + UNREADABLE_AFTER_MS);
    assert_eq!(
        gate(&checking_since(Kind::Prompt, 10_000), &f),
        Gate::Wait {
            why: Why::Unreadable,
            until_ms: 10_000 + UNREADABLE_AFTER_MS + 15_000
        }
    );
    f.now_ms += 3_600_000;
    assert!(matches!(
        gate(&checking_since(Kind::Command, 10_000), &f),
        Gate::Wait {
            why: Why::Unreadable,
            ..
        }
    ));
}

#[test]
fn the_unknown_wait_is_the_rows_own_and_starts_again_when_the_row_is_put_back_in_line() {
    let f = unknown(10_000 + UNREADABLE_AFTER_MS + 5_000);
    let waiting = checking_since(Kind::Prompt, 10_000);
    assert!(
        matches!(
            gate(&waiting, &f),
            Gate::Wait {
                why: Why::Unreadable,
                ..
            }
        ),
        "the start is the row's updated_ms, so a daemon restart keeps it"
    );
    let queued = Row {
        state: State::Queued,
        ..waiting.clone()
    };
    let retry = Row {
        why: Some(Why::Retry),
        ..waiting.clone()
    };
    let ahead = Row {
        updated_ms: f.now_ms + 60_000,
        ..waiting
    };
    for (name, row) in [("queued", queued), ("retry", retry), ("clock", ahead)] {
        assert_eq!(
            gate(&row, &f),
            Gate::Wait {
                why: Why::Checking,
                until_ms: f.now_ms + 1_000
            },
            "{name}"
        );
    }
}

#[test]
fn an_unknown_job_is_looked_at_less_often_the_longer_it_lasts() {
    let since = 10_000;
    let until = |age: i64| {
        let f = unknown(since + age);
        match gate(&checking_since(Kind::Prompt, since), &f) {
            Gate::Wait { until_ms, .. } => until_ms - (since + age),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(until(0), 1_000);
    assert_eq!(until(4_999), 1_000);
    assert_eq!(until(5_000), 2_000);
    assert_eq!(until(15_000), 5_000);
    assert_eq!(until(UNREADABLE_AFTER_MS), 15_000);
    assert_eq!(until(60_000), 30_000);
    assert_eq!(until(3_600_000), 30_000);
}

#[test]
fn an_unknown_job_is_looked_at_again_when_it_turns_unreadable() {
    let since = 10_000;
    let f = unknown(since + UNREADABLE_AFTER_MS - 1_000);
    assert_eq!(
        gate(&checking_since(Kind::Prompt, since), &f),
        Gate::Wait {
            why: Why::Checking,
            until_ms: since + UNREADABLE_AFTER_MS
        }
    );
}

#[test]
fn the_done_count_starts_on_done_is_dropped_on_any_other_job_and_kept_while_unknown() {
    let mut r = row(Kind::Prompt);
    r.tries = 4;
    assert_eq!(recount(&r, &Job::Done), Some(true));
    assert_eq!(recount(&r, &Job::Unknown), None);
    assert_eq!(recount(&r, &Job::Stopped), None);
    r.done_from = Some(4);
    assert_eq!(recount(&r, &Job::Done), None);
    assert_eq!(recount(&r, &Job::Unknown), None);
    assert_eq!(recount(&r, &Job::Live { status: None }), Some(false));
    assert_eq!(recount(&r, &Job::Missing { since_ms: 0 }), Some(false));
    r.done_from = Some(9);
    assert_eq!(recount(&r, &Job::Done), Some(true));
}

#[test]
fn a_done_job_is_stopped_only_after_the_tries_counted_on_the_row_fail() {
    let f = Facts {
        job: Job::Done,
        ..facts(1_000)
    };
    let go = Gate::Go { guarded: false };
    let mut r = row(Kind::Prompt);
    r.state = State::Waiting;
    r.tries = 9;
    assert_eq!(
        gate(&r, &f),
        go,
        "tries before the job was done count for nothing"
    );
    r.done_from = Some(9);
    assert_eq!(gate(&r, &f), go);
    r.tries = 9 + DONE_STOPS_AFTER_TRIES - 1;
    assert_eq!(gate(&r, &f), go);
    r.tries = 9 + DONE_STOPS_AFTER_TRIES;
    assert_eq!(gate(&r, &f), Gate::NeedsUser(State::Stopped));
    r.done_from = Some(40);
    assert_eq!(
        gate(&r, &f),
        go,
        "a count above the tries never stops a row"
    );
}

#[test]
fn a_row_put_back_in_line_starts_its_done_count_again_whoever_put_it_back() {
    let f = Facts {
        job: Job::Done,
        ..facts(1_000)
    };
    let mut r = row(Kind::Prompt);
    r.tries = DONE_STOPS_AFTER_TRIES;
    r.done_from = Some(0);
    assert_eq!(r.state, State::Queued);
    assert_eq!(
        recount(&r, &Job::Done),
        Some(true),
        "a count an older daemon left on a row it put back is taken again"
    );
    assert_eq!(
        gate(&r, &f),
        Gate::Go { guarded: false },
        "and never stops the row untried"
    );
    r.state = State::Waiting;
    assert_eq!(recount(&r, &Job::Done), None, "a waiting row keeps it");
    assert_eq!(gate(&r, &f), Gate::NeedsUser(State::Stopped));
}

#[test]
fn a_row_has_moved_when_its_state_time_tries_or_words_changed() {
    let before = row(Kind::Prompt);
    assert!(before.clone().unmoved_since(&before));
    let mut counted = before.clone();
    counted.done_from = Some(2);
    assert!(counted.unmoved_since(&before), "the done count is no move");
    let changes: [fn(&mut Row); 5] = [
        |r| r.state = State::Waiting,
        |r| r.why = Some(Why::Retry),
        |r| r.updated_ms += 1,
        |r| r.tries += 1,
        |r| r.text.push('!'),
    ];
    for (n, change) in changes.iter().enumerate() {
        let mut after = before.clone();
        change(&mut after);
        assert!(!after.unmoved_since(&before), "{n}");
    }
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
