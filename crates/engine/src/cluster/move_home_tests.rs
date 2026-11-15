use super::id::NodeId;
use super::move_home::{
    failure_notice, left_behind, next, prepare, resume_call_ms, step_name, unsaved, FinalSet,
    MoveAction, MoveEvent, MoveFacts, MoveSaga, MoveStep, StopResult, SwitchResult, ToFacts,
    FREEZE_WAIT_MS, NO_PROOF, RESUME_CAP_MS, SWITCH_NOTICE_TRIES,
};
use super::snapshot::Movable;
use super::testkit::id;
use super::versions::Compat;

fn ids() -> (NodeId, NodeId) {
    (id("m5"), id("m1"))
}

fn fin(left: Vec<(String, String)>) -> FinalSet {
    FinalSet {
        set: "s-final".to_string(),
        left,
        at_ms: 4_000,
    }
}

fn to_facts() -> ToFacts {
    ToFacts {
        name: "m1".to_string(),
        member: true,
        can_be_master: true,
        online: true,
        compat: Compat::Ok,
        ready: Ok(()),
    }
}

fn facts() -> MoveFacts {
    MoveFacts {
        term: 3,
        is_home: true,
        to: to_facts(),
        main: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        set: Some(vec![
            ("a".to_string(), Movable::Yes),
            (
                "b".to_string(),
                Movable::No("repo has unpushed commits".to_string()),
            ),
            ("c".to_string(), Movable::Yes),
        ]),
    }
}

fn started(now: bool) -> MoveSaga {
    let (from, to) = ids();
    MoveSaga::start(from, to, 1_000, now).0
}

fn drive(saga: &MoveSaga, events: Vec<MoveEvent>) -> (MoveSaga, Vec<MoveAction>) {
    let mut saga = saga.clone();
    let mut last = Vec::new();
    for event in events {
        let (after, actions) = next(&saga, event, true);
        saga = after;
        last = actions;
    }
    (saga, last)
}

fn to_freeze() -> MoveSaga {
    drive(
        &started(false),
        vec![
            MoveEvent::Checked(facts()),
            MoveEvent::CaughtUp(Ok(())),
            MoveEvent::Frozen { now_ms: 2_000 },
        ],
    )
    .0
}

fn to_switch() -> MoveSaga {
    drive(
        &to_freeze(),
        vec![
            MoveEvent::Working {
                now_ms: 3_000,
                working: Vec::new(),
            },
            MoveEvent::Final(Ok(fin(Vec::new()))),
            MoveEvent::Stopped(StopResult {
                done: vec!["a".to_string(), "c".to_string()],
                failed: None,
            }),
        ],
    )
    .0
}

#[test]
fn prepare_needs_proof() {
    let (from, to) = ids();
    let err = prepare(&from, &to, &facts(), false).unwrap_err();
    assert_eq!(err, "Moving the primary needs the resume proof first");
    assert_eq!(err, NO_PROOF);
    assert!(prepare(&from, &to, &facts(), true).is_ok());

    let (saga, actions) = next(&started(false), MoveEvent::Checked(facts()), false);
    assert_eq!(saga.step, MoveStep::Undo(NO_PROOF.to_string()));
    assert_eq!(
        actions.last(),
        Some(&MoveAction::Notice(failure_notice(NO_PROOF)))
    );
}

#[test]
fn prepare_refuses_a_target_that_cannot_take_over() {
    let (from, to) = ids();
    let mut offline = facts();
    offline.to.online = false;
    assert_eq!(
        prepare(&from, &to, &offline, true).unwrap_err(),
        "m1 is offline"
    );
    let mut off = facts();
    off.to.can_be_master = false;
    assert!(prepare(&from, &to, &off, true).is_err());
    let mut old = facts();
    old.to.compat = Compat::Refuse("m1 runs Notchling 0.9".to_string());
    assert_eq!(
        prepare(&from, &to, &old, true).unwrap_err(),
        "m1 runs Notchling 0.9"
    );
    let mut warned = facts();
    warned.to.compat = Compat::Warn("careful".to_string());
    assert!(prepare(&from, &to, &warned, true).is_err());
    let mut unready = facts();
    unready.to.ready = Err("disk encryption is off".to_string());
    assert_eq!(
        prepare(&from, &to, &unready, true).unwrap_err(),
        "m1 isn't ready: disk encryption is off"
    );
    let mut stranger = facts();
    stranger.to.member = false;
    assert!(prepare(&from, &to, &stranger, true).is_err());
    let mut backup = facts();
    backup.is_home = false;
    assert!(prepare(&from, &to, &backup, true).is_err());
    assert!(prepare(&from, &from, &facts(), true).is_err());
}

#[test]
fn prepare_splits_moving_and_staying() {
    let (from, to) = ids();
    let prepared = prepare(&from, &to, &facts(), true).unwrap();
    assert_eq!(prepared.moving, vec!["a".to_string(), "c".to_string()]);
    assert_eq!(
        prepared.staying,
        vec![("b".to_string(), "repo has unpushed commits".to_string())]
    );

    let mut fresh = facts();
    fresh.main.push("d".to_string());
    let prepared = prepare(&from, &to, &fresh, true).unwrap();
    assert!(prepared.moving.contains(&"d".to_string()));

    let mut empty = facts();
    empty.set = None;
    let prepared = prepare(&from, &to, &empty, true).unwrap();
    assert_eq!(prepared.moving.len(), 3);
    assert!(prepared.staying.is_empty());

    let (saga, actions) = next(&started(false), MoveEvent::Checked(facts()), true);
    assert_eq!(saga.step, MoveStep::CatchUp);
    assert_eq!(saga.moving, prepared_moving());
    assert_eq!(saga.staying.len(), 1);
    assert_eq!(saga.term, 3);
    assert_eq!(actions, vec![MoveAction::CatchUp]);
}

fn prepared_moving() -> Vec<String> {
    vec!["a".to_string(), "c".to_string()]
}

#[test]
fn freeze_waits_for_turn_ends() {
    let frozen = to_freeze();
    assert_eq!(frozen.step, MoveStep::Freeze);
    assert_eq!(frozen.froze_ms, Some(2_000));

    let (still, actions) = next(
        &frozen,
        MoveEvent::Working {
            now_ms: 60_000,
            working: vec!["a".to_string()],
        },
        true,
    );
    assert_eq!(still.step, MoveStep::Freeze);
    assert_eq!(actions, vec![MoveAction::Poll]);

    let outsider = next(
        &frozen,
        MoveEvent::Working {
            now_ms: 60_000,
            working: vec!["b".to_string()],
        },
        true,
    );
    assert_eq!(outsider.0.step, MoveStep::FinalSnapshot);

    let (done, actions) = next(
        &still,
        MoveEvent::Working {
            now_ms: 70_000,
            working: Vec::new(),
        },
        true,
    );
    assert_eq!(done.step, MoveStep::FinalSnapshot);
    assert_eq!(actions, vec![MoveAction::Fence, MoveAction::FinalSnapshot]);

    let (late, actions) = next(
        &still,
        MoveEvent::Working {
            now_ms: 2_000 + FREEZE_WAIT_MS,
            working: vec!["a".to_string()],
        },
        true,
    );
    assert!(matches!(late.step, MoveStep::Undo(ref why) if why.contains("still working")));
    assert!(actions.contains(&MoveAction::Unfreeze));
}

#[test]
fn move_now_ends_the_wait_and_names_cut_sessions() {
    let frozen = to_freeze();
    let (cut, actions) = next(
        &frozen,
        MoveEvent::MoveNow {
            working: vec!["a".to_string()],
        },
        true,
    );
    assert!(cut.now);
    assert_eq!(cut.cut, vec!["a".to_string()]);
    assert_eq!(cut.step, MoveStep::Freeze);
    assert_eq!(
        actions,
        vec![MoveAction::StopCut {
            sessions: vec!["a".to_string()]
        }]
    );

    let (after, actions) = next(
        &cut,
        MoveEvent::Cut(StopResult {
            done: vec!["a".to_string()],
            failed: None,
        }),
        true,
    );
    assert_eq!(after.step, MoveStep::FinalSnapshot);
    assert_eq!(after.cut, vec!["a".to_string()]);
    assert_eq!(after.stopped, vec!["a".to_string()]);
    assert_eq!(actions, vec![MoveAction::Fence, MoveAction::FinalSnapshot]);

    let (nothing, actions) = next(
        &frozen,
        MoveEvent::MoveNow {
            working: Vec::new(),
        },
        true,
    );
    assert_eq!(nothing.step, MoveStep::FinalSnapshot);
    assert!(nothing.cut.is_empty());
    assert_eq!(actions, vec![MoveAction::Fence, MoveAction::FinalSnapshot]);

    let early = next(
        &started(false),
        MoveEvent::MoveNow { working: vec![] },
        true,
    );
    assert!(early.0.now);
    assert!(early.1.is_empty());

    let (failed, actions) = next(
        &cut,
        MoveEvent::Cut(StopResult {
            done: Vec::new(),
            failed: Some(("a".to_string(), "claude stop timed out".to_string())),
        }),
        true,
    );
    assert!(matches!(failed.step, MoveStep::Undo(_)));
    assert!(actions.contains(&MoveAction::Unfreeze));
}

#[test]
fn session_unmovable_in_final_set_stays() {
    let set = vec![
        ("a".to_string(), Movable::Yes),
        ("c".to_string(), Movable::No("repo changed".to_string())),
    ];
    let left = left_behind(&["a".to_string(), "c".to_string()], &set);
    assert_eq!(left, vec![("c".to_string(), "repo changed".to_string())]);
    let missing = left_behind(&["a".to_string(), "z".to_string()], &set);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].0, "z");

    let at_final = drive(
        &to_freeze(),
        vec![MoveEvent::Working {
            now_ms: 3_000,
            working: Vec::new(),
        }],
    )
    .0;
    let (saga, actions) = next(
        &at_final,
        MoveEvent::Final(Ok(fin(vec![("c".to_string(), "repo changed".to_string())]))),
        true,
    );
    assert_eq!(saga.moving, vec!["a".to_string()]);
    assert_eq!(saga.staying.len(), 2);
    assert!(saga
        .staying
        .contains(&("c".to_string(), "repo changed".to_string())));
    assert_eq!(saga.step, MoveStep::StopOld);
    assert_eq!(saga.set.as_deref(), Some("s-final"));
    assert_eq!(saga.final_ms, Some(4_000));
    assert_eq!(
        actions,
        vec![
            MoveAction::HoldSets,
            MoveAction::StopOld {
                sessions: vec!["a".to_string()]
            }
        ]
    );
}

#[test]
fn failure_before_switch_compensates() {
    let reason = "m1 didn't answer";
    let (caught, _) = drive(&started(false), vec![MoveEvent::Checked(facts())]);
    let (after, actions) = next(&caught, MoveEvent::CaughtUp(Err(reason.to_string())), true);
    assert!(matches!(after.step, MoveStep::Undo(_)));
    assert_eq!(actions.first(), Some(&MoveAction::Unfreeze));
    assert_eq!(
        actions.last(),
        Some(&MoveAction::Notice(failure_notice(reason)))
    );

    let at_final = drive(
        &to_freeze(),
        vec![MoveEvent::Working {
            now_ms: 3_000,
            working: Vec::new(),
        }],
    )
    .0;
    assert_eq!(at_final.step, MoveStep::FinalSnapshot);
    let (after, actions) = next(&at_final, MoveEvent::Final(Err(reason.to_string())), true);
    assert!(matches!(after.step, MoveStep::Undo(_)));
    assert!(actions.contains(&MoveAction::Unfreeze));
    assert!(!actions
        .iter()
        .any(|a| matches!(a, MoveAction::Restart { .. })));

    let at_stop = drive(
        &to_freeze(),
        vec![
            MoveEvent::Working {
                now_ms: 3_000,
                working: Vec::new(),
            },
            MoveEvent::Final(Ok(fin(Vec::new()))),
        ],
    )
    .0;
    assert_eq!(at_stop.step, MoveStep::StopOld);
    let (after, actions) = next(
        &at_stop,
        MoveEvent::Stopped(StopResult {
            done: vec!["a".to_string()],
            failed: Some(("c".to_string(), "claude stop failed".to_string())),
        }),
        true,
    );
    assert!(matches!(after.step, MoveStep::Undo(_)));
    assert_eq!(
        actions,
        vec![
            MoveAction::Unfreeze,
            MoveAction::Restart {
                sessions: vec!["a".to_string()],
                unless_live: vec!["c".to_string()],
            },
            MoveAction::Notice(failure_notice("c didn't stop: claude stop failed")),
        ]
    );

    let (restarted, actions) = next(&to_freeze(), MoveEvent::Restarted, true);
    assert!(matches!(restarted.step, MoveStep::Undo(_)));
    assert!(actions.contains(&MoveAction::Unfreeze));

    let switch = to_switch();
    let (refused, actions) = next(
        &switch,
        MoveEvent::Switched(SwitchResult::Refused("m1 has a newer term".to_string())),
        true,
    );
    assert!(matches!(refused.step, MoveStep::Undo(_)));
    assert!(actions
        .iter()
        .any(|a| matches!(a, MoveAction::Restart { sessions, .. } if sessions.len() == 2)));
}

#[test]
fn switch_bumps_term_once() {
    let switch = to_switch();
    assert_eq!(switch.step, MoveStep::Switch);
    let (_, first) = drive(
        &to_freeze(),
        vec![
            MoveEvent::Working {
                now_ms: 3_000,
                working: Vec::new(),
            },
            MoveEvent::Final(Ok(fin(Vec::new()))),
            MoveEvent::Stopped(StopResult {
                done: vec!["a".to_string(), "c".to_string()],
                failed: None,
            }),
        ],
    );
    assert_eq!(
        first,
        vec![MoveAction::Switch {
            term: 4,
            attempt: 0
        }]
    );

    let (lost, again) = next(
        &switch,
        MoveEvent::Switched(SwitchResult::Lost("timed out".to_string())),
        true,
    );
    assert_eq!(lost.step, MoveStep::Switch);
    assert_eq!(
        again,
        vec![MoveAction::Switch {
            term: 4,
            attempt: 1
        }]
    );

    let (done, actions) = next(&lost, MoveEvent::Switched(SwitchResult::Done), true);
    assert_eq!(done.step, MoveStep::Resume);
    assert_eq!(done.term, 3);
    assert!(!actions
        .iter()
        .any(|a| matches!(a, MoveAction::Switch { .. })));

    let (restart, actions) = next(&switch, MoveEvent::Restarted, true);
    assert_eq!(restart.step, MoveStep::Switch);
    assert!(restart.maybe_sent);
    assert_eq!(
        actions,
        vec![
            MoveAction::Guard,
            MoveAction::Switch {
                term: 4,
                attempt: 0
            }
        ]
    );

    let mut long = switch;
    long.tries = SWITCH_NOTICE_TRIES - 1;
    let (_, actions) = next(
        &long,
        MoveEvent::Switched(SwitchResult::Lost("offline".to_string())),
        true,
    );
    assert!(actions
        .iter()
        .any(|a| matches!(a, MoveAction::Notice(text) if text.contains("m1"))));
}

#[test]
fn no_undo_after_switch() {
    let resumed = next(&to_switch(), MoveEvent::Switched(SwitchResult::Done), true).0;
    assert_eq!(resumed.step, MoveStep::Resume);
    let (after, actions) = next(&resumed, MoveEvent::CaughtUp(Err("late".to_string())), true);
    assert_eq!(after, resumed);
    assert!(actions.is_empty());

    let (restarted, actions) = next(&resumed, MoveEvent::Restarted, true);
    assert_eq!(restarted.step, MoveStep::Resume);
    assert!(!actions.contains(&MoveAction::Unfreeze));
    assert!(matches!(
        actions.as_slice(),
        [MoveAction::Guard, MoveAction::Resume { .. }]
    ));

    let (failed, actions) = next(
        &resumed,
        MoveEvent::Resumed(vec![
            ("a".to_string(), Ok(())),
            ("c".to_string(), Err("claude --bg failed".to_string())),
        ]),
        true,
    );
    assert_eq!(failed.step, MoveStep::Announce);
    assert_eq!(
        failed.failed,
        vec![("c".to_string(), "claude --bg failed".to_string())]
    );
    assert_eq!(
        actions,
        vec![
            MoveAction::ResumeFailed {
                session: "c".to_string(),
                why: "claude --bg failed".to_string()
            },
            MoveAction::Announce,
        ]
    );
    assert!(actions
        .iter()
        .all(|a| !matches!(a, MoveAction::Unfreeze | MoveAction::Restart { .. })));

    let (done, actions) = next(&failed, MoveEvent::Announced, true);
    assert_eq!(done.step, MoveStep::Done);
    assert!(done.finished());
    assert_eq!(actions, vec![MoveAction::Unfreeze]);
    let (same, _) = next(&done, MoveEvent::Restarted, true);
    assert_eq!(same, done);
}

#[test]
fn resume_never_sends_a_message() {
    let resumed = next(&to_switch(), MoveEvent::Switched(SwitchResult::Done), true);
    let sessions = vec!["a".to_string(), "c".to_string()];
    assert_eq!(resumed.1, vec![MoveAction::Resume { sessions }]);
    for action in resumed.1 {
        match action {
            MoveAction::Resume { sessions } => assert_eq!(sessions.len(), 2),
            MoveAction::Check
            | MoveAction::CatchUp
            | MoveAction::Freeze { .. }
            | MoveAction::Poll
            | MoveAction::StopCut { .. }
            | MoveAction::Fence
            | MoveAction::FinalSnapshot
            | MoveAction::HoldSets
            | MoveAction::StopOld { .. }
            | MoveAction::Switch { .. }
            | MoveAction::Announce
            | MoveAction::Guard
            | MoveAction::Unfreeze
            | MoveAction::Restart { .. }
            | MoveAction::Notice(_)
            | MoveAction::ResumeFailed { .. } => unreachable!(),
        }
    }
}

#[test]
fn a_saga_survives_the_journal() {
    let saga = to_switch();
    let text = serde_json::to_string(&saga).unwrap();
    let back: MoveSaga = serde_json::from_str(&text).unwrap();
    assert_eq!(back, saga);
}

fn undone(saga: &MoveSaga) -> bool {
    matches!(saga.step, MoveStep::Undo(_))
}

#[test]
fn h1_a_refusal_after_a_lost_answer_rolls_forward() {
    let (lost, _) = next(
        &to_switch(),
        MoveEvent::Switched(SwitchResult::Lost("the link dropped".to_string())),
        true,
    );
    assert!(lost.maybe_sent);
    for answer in [
        SwitchResult::NotSent("m1 is offline".to_string()),
        SwitchResult::Refused("this machine already has a newer term".to_string()),
    ] {
        let (after, actions) = next(&lost, MoveEvent::Switched(answer), true);
        assert!(!undone(&after), "{after:?}");
        assert_eq!(after.step, MoveStep::Switch);
        assert_eq!(
            actions,
            vec![MoveAction::Switch {
                term: 4,
                attempt: 2
            }]
        );
        assert!(!actions
            .iter()
            .any(|a| matches!(a, MoveAction::Restart { .. } | MoveAction::Unfreeze)));
    }
}

#[test]
fn h1_a_restart_at_switch_never_undoes() {
    let (back, _) = next(&to_switch(), MoveEvent::Restarted, true);
    for answer in [
        SwitchResult::NotSent("the cluster node isn't running".to_string()),
        SwitchResult::Refused("that term isn't this machine's".to_string()),
    ] {
        let (after, actions) = next(&back, MoveEvent::Switched(answer), true);
        assert_eq!(after.step, MoveStep::Switch);
        assert!(matches!(actions.as_slice(), [MoveAction::Switch { .. }]));
    }
}

#[test]
fn h1_only_a_first_answered_refusal_or_nothing_sent_undoes() {
    for answer in [
        SwitchResult::Refused("m1 can't be primary".to_string()),
        SwitchResult::NotSent("m1 is offline".to_string()),
    ] {
        let (after, actions) = next(&to_switch(), MoveEvent::Switched(answer), true);
        assert!(undone(&after));
        assert!(actions.contains(&MoveAction::Unfreeze));
    }
    let (after, _) = next(
        &to_switch(),
        MoveEvent::Switched(SwitchResult::Lost("timed out".to_string())),
        true,
    );
    assert!(!undone(&after));
}

#[test]
fn h3_a_cut_session_left_behind_by_the_final_set_starts_again() {
    let (cut, _) = next(
        &to_freeze(),
        MoveEvent::MoveNow {
            working: vec!["a".to_string()],
        },
        true,
    );
    let (at_final, _) = next(
        &cut,
        MoveEvent::Cut(StopResult {
            done: vec!["a".to_string()],
            failed: None,
        }),
        true,
    );
    let why = "It has commits that aren't pushed".to_string();
    let (saga, actions) = next(
        &at_final,
        MoveEvent::Final(Ok(fin(vec![("a".to_string(), why.clone())]))),
        true,
    );
    assert!(saga.staying.contains(&("a".to_string(), why)));
    assert!(!saga.stopped.contains(&"a".to_string()));
    assert!(!saga.moving.contains(&"a".to_string()));
    assert_eq!(
        actions,
        vec![
            MoveAction::HoldSets,
            MoveAction::Restart {
                sessions: vec!["a".to_string()],
                unless_live: Vec::new(),
            },
            MoveAction::StopOld {
                sessions: vec!["c".to_string()]
            },
        ]
    );
}

#[test]
fn m1_a_stop_in_flight_restarts_only_if_it_is_down() {
    let at_stop = drive(
        &to_freeze(),
        vec![
            MoveEvent::Working {
                now_ms: 3_000,
                working: Vec::new(),
            },
            MoveEvent::Final(Ok(fin(Vec::new()))),
        ],
    )
    .0;
    let mut journaled = at_stop.clone();
    journaled.mark_stopping("a");
    journaled.mark_stopped("a");
    journaled.mark_stopping("c");
    assert_eq!(journaled.stopped, vec!["a".to_string()]);
    assert_eq!(journaled.stopping, vec!["c".to_string()]);
    let back: MoveSaga = serde_json::from_str(&serde_json::to_string(&journaled).unwrap()).unwrap();
    let (after, actions) = next(&back, MoveEvent::Restarted, true);
    assert!(undone(&after));
    assert!(actions.contains(&MoveAction::Restart {
        sessions: vec!["a".to_string()],
        unless_live: vec!["c".to_string()],
    }));
}

#[test]
fn m7_the_old_primary_is_fenced_from_the_final_snapshot_to_the_end() {
    let (at_final, actions) = next(
        &to_freeze(),
        MoveEvent::Working {
            now_ms: 3_000,
            working: Vec::new(),
        },
        true,
    );
    assert_eq!(actions.first(), Some(&MoveAction::Fence));
    assert!(at_final.fenced());
    assert!(!to_freeze().fenced());
    assert!(to_switch().fenced());
    let resumed = next(&to_switch(), MoveEvent::Switched(SwitchResult::Done), true).0;
    let announced = next(&resumed, MoveEvent::Resumed(Vec::new()), true).0;
    assert!(announced.fenced());
    let (done, actions) = next(&announced, MoveEvent::Announced, true);
    assert!(!done.fenced());
    assert_eq!(actions, vec![MoveAction::Unfreeze]);
}

#[test]
fn l3_a_switch_that_wasnt_journaled_is_never_sent() {
    let at_stop = drive(
        &to_freeze(),
        vec![
            MoveEvent::Working {
                now_ms: 3_000,
                working: Vec::new(),
            },
            MoveEvent::Final(Ok(fin(Vec::new()))),
        ],
    )
    .0;
    let (switch, _) = next(
        &at_stop,
        MoveEvent::Stopped(StopResult {
            done: vec!["a".to_string(), "c".to_string()],
            failed: None,
        }),
        true,
    );
    assert_eq!(switch.step, MoveStep::Switch);
    let (after, actions) = unsaved(&at_stop, &switch, "disk full").expect("an undo");
    assert!(undone(&after));
    assert!(!actions
        .iter()
        .any(|a| matches!(a, MoveAction::Switch { .. })));
    assert!(actions
        .iter()
        .any(|a| matches!(a, MoveAction::Restart { sessions, .. } if sessions.len() == 2)));
    let (lost, _) = next(
        &switch,
        MoveEvent::Switched(SwitchResult::Lost("timed out".to_string())),
        true,
    );
    assert!(unsaved(&switch, &lost, "disk full").is_none());
}

#[test]
fn l4_step_names_are_snake_case() {
    assert_eq!(step_name(&MoveStep::CatchUp), "catch_up");
    assert_eq!(step_name(&MoveStep::FinalSnapshot), "final_snapshot");
    assert_eq!(step_name(&MoveStep::StopOld), "stop_old");
    assert_eq!(step_name(&MoveStep::Undo("x".to_string())), "undone");
}

#[test]
fn m2_the_resume_wait_grows_with_the_sessions_and_is_capped() {
    assert!(resume_call_ms(1) >= 60_000);
    assert!(resume_call_ms(4) > resume_call_ms(1) + 3 * 60_000);
    assert_eq!(resume_call_ms(10_000), RESUME_CAP_MS);
}
