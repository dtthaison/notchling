use super::*;

fn row(why: Option<Why>, updated_ms: i64) -> Row {
    Row {
        n: 1,
        id: "id".into(),
        session: "s".into(),
        tool: Tool::Claude,
        kind: Kind::Command,
        text: "/model claude-opus-5-5".into(),
        options: None,
        via: Via::Chat,
        state: State::Waiting,
        why,
        clear_box: false,
        queued_ms: 0,
        updated_ms,
        attempt_ms: None,
        seen_before: None,
        queued_before: None,
        tries: 1,
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

fn facts(now_ms: i64, status: &str) -> Facts {
    Facts {
        now_ms,
        job: Job::Live {
            status: Some(status.into()),
        },
        dialog_on_screen: false,
        queue_waiting: 0,
        safe: false,
        turn_over_since_ms: None,
    }
}

#[test]
fn a_row_that_asked_something_waits_while_the_session_waits() {
    let asked = row(Some(Why::Answer), 1_000);
    let f = facts(1_000 + ANSWER_GRACE_MS + 40_000, "waiting");
    assert_eq!(
        gate(&asked, &f),
        Gate::Wait {
            why: Why::Answer,
            until_ms: f.now_ms + 1_000
        }
    );
}

#[test]
fn it_is_given_a_moment_before_the_session_is_believed_to_be_done_with_it() {
    let asked = row(Some(Why::Answer), 1_000);
    let f = facts(1_000 + ANSWER_GRACE_MS - 1, "idle");
    assert!(matches!(
        gate(&asked, &f),
        Gate::Wait {
            why: Why::Answer,
            ..
        }
    ));
}

#[test]
fn it_is_delivered_once_the_session_stopped_waiting() {
    let asked = row(Some(Why::Answer), 1_000);
    for status in ["idle", "busy"] {
        let f = facts(1_000 + ANSWER_GRACE_MS, status);
        assert_eq!(gate(&asked, &f), Gate::Delivered, "{status}");
    }
}

#[test]
fn it_is_never_typed_again() {
    let asked = row(Some(Why::Answer), 0);
    for status in ["waiting", "idle", "busy", "shell"] {
        for now in [0, 2_000, 60_000] {
            let go = matches!(gate(&asked, &facts(now, status)), Gate::Go { .. });
            assert!(!go, "{status} at {now}");
        }
    }
}

#[test]
fn a_stopped_session_still_ends_the_wait_the_usual_way() {
    let asked = row(Some(Why::Answer), 0);
    let mut f = facts(100_000, "waiting");
    f.job = Job::Stopped;
    assert_eq!(gate(&asked, &f), Gate::NeedsUser(State::Stopped));
}

#[test]
fn an_asked_command_comes_back_to_waiting_for_the_answer() {
    let tried = Tried {
        pasted: true,
        entered: true,
    };
    assert_eq!(
        after(&TryEnd::Asked, tried, 1, 5_000),
        After::Wait {
            why: Why::Answer,
            next_ms: None
        }
    );
}

#[test]
fn it_is_kept_while_a_dialog_is_still_on_screen_whatever_the_job_says() {
    let asked = row(Some(Why::Answer), 1_000);
    for status in ["idle", "busy", "waiting"] {
        let mut f = facts(1_000 + ANSWER_GRACE_MS + 60_000, status);
        f.dialog_on_screen = true;
        assert_eq!(
            gate(&asked, &f),
            Gate::Wait {
                why: Why::Answer,
                until_ms: f.now_ms + 1_000
            },
            "{status}"
        );
        f.dialog_on_screen = false;
        if status != "waiting" {
            assert_eq!(gate(&asked, &f), Gate::Delivered, "{status}");
        }
    }
}
