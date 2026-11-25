use super::*;
use crate::reply::exec::Failure;

const NONE: Tried = Tried {
    pasted: false,
    entered: false,
};
const PASTED: Tried = Tried {
    pasted: true,
    entered: false,
};
const ENTERED: Tried = Tried {
    pasted: true,
    entered: true,
};

fn all_failures() -> Vec<Failure> {
    vec![
        Failure::Pty("x".into()),
        Failure::Timeout,
        Failure::Dialog,
        Failure::NoInput,
        Failure::NotCleared,
        Failure::NotTaken,
        Failure::NotSent,
        Failure::Mixed,
        Failure::QuestionGone,
        Failure::QuestionMismatch,
        Failure::NotPermission,
        Failure::PermissionMismatch,
        Failure::NoAlways,
        Failure::PermissionStillOpen,
    ]
}

#[test]
fn ok_verifies() {
    assert_eq!(after(&TryEnd::Ok, ENTERED, 1, 0), After::Verify);
}

#[test]
fn dialog_before_paste_waits_dialog() {
    assert_eq!(
        after(&TryEnd::Failed(Failure::Dialog), NONE, 1, 0),
        After::Wait {
            why: Why::Dialog,
            next_ms: None
        }
    );
}

#[test]
fn a_signed_out_failure_waits_for_the_recheck_and_not_for_an_instant_retry() {
    for tried in [NONE, PASTED] {
        for tries in [0, 1, 5] {
            assert_eq!(
                after(&TryEnd::Failed(Failure::LoginNeeded), tried, tries, 7_000),
                After::Wait {
                    why: Why::LoginNeeded,
                    next_ms: Some(7_000 + LOGIN_RECHECK_MS)
                }
            );
        }
    }
}

#[test]
fn not_cleared_is_box() {
    assert_eq!(
        after(&TryEnd::Failed(Failure::NotCleared), NONE, 1, 0),
        After::Box(Why::NotCleared)
    );
}

#[test]
fn mixed_is_box_mixed() {
    assert_eq!(
        after(&TryEnd::Failed(Failure::Mixed), PASTED, 1, 0),
        After::Box(Why::Mixed)
    );
}

#[test]
fn not_sent_after_enter_verifies() {
    assert_eq!(
        after(&TryEnd::Failed(Failure::NotSent), ENTERED, 1, 0),
        After::Verify
    );
}

#[test]
fn timeout_after_enter_verifies() {
    assert_eq!(
        after(&TryEnd::Failed(Failure::Timeout), ENTERED, 1, 0),
        After::Verify
    );
    assert_eq!(
        after(&TryEnd::Failed(Failure::Pty("gone".into())), ENTERED, 1, 0),
        After::Verify
    );
}

#[test]
fn not_idle_waits_turn() {
    assert_eq!(
        after(&TryEnd::NotIdle, NONE, 1, 0),
        After::Wait {
            why: Why::Turn,
            next_ms: None
        }
    );
}

#[test]
fn no_input_attach_failed_and_not_taken_retry_with_backoff() {
    let ends = [
        TryEnd::Failed(Failure::NoInput),
        TryEnd::Failed(Failure::NotTaken),
        TryEnd::AttachFailed,
    ];
    for end in &ends {
        for (tries, ms) in [(1, 30_000), (2, 60_000), (3, 120_000), (9, 300_000)] {
            assert_eq!(
                after(end, PASTED, tries, 1_000),
                After::Wait {
                    why: Why::Retry,
                    next_ms: Some(1_000 + ms)
                }
            );
        }
    }
    assert_eq!(backoff_ms(0), 30_000);
    assert_eq!(backoff_ms(4), 300_000);
}

#[test]
fn nothing_before_enter_is_ever_unsure() {
    for failure in all_failures() {
        for tried in [NONE, PASTED] {
            let got = after(&TryEnd::Failed(failure.clone()), tried, 1, 0);
            assert!(
                matches!(got, After::Box(_) | After::Wait { .. }),
                "{failure:?} {tried:?} -> {got:?}"
            );
        }
    }
    for end in [TryEnd::NotIdle, TryEnd::AttachFailed] {
        let got = after(&end, NONE, 1, 0);
        assert!(matches!(got, After::Wait { .. }));
    }
}

#[test]
fn any_end_after_enter_verifies() {
    for end in [TryEnd::NotIdle, TryEnd::AttachFailed] {
        assert_eq!(after(&end, ENTERED, 1, 0), After::Verify, "{end:?}");
    }
    for failure in all_failures() {
        assert_eq!(
            after(&TryEnd::Failed(failure.clone()), ENTERED, 1, 0),
            After::Verify,
            "{failure:?}"
        );
    }
}

#[test]
fn cut_after_start_is_unsure_restart_never_retried() {
    for tried in [NONE, PASTED, ENTERED] {
        for tries in [0, 1, 5] {
            assert_eq!(
                after(&TryEnd::Cut, tried, tries, 1_000),
                After::Unsure(Why::Restart),
                "{tried:?} {tries}"
            );
        }
    }
}
