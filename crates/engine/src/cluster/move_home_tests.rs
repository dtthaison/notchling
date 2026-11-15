use super::id::NodeId;
use super::move_home::{
    failure_notice, left_behind, next, prepare, MoveAction, MoveEvent, MoveFacts, MoveSaga,
    MoveStep, StopResult, SwitchResult, ToFacts, FREEZE_WAIT_MS, NO_PROOF, SWITCH_NOTICE_TRIES,
};
use super::snapshot::Movable;
use super::testkit::id;
use super::versions::Compat;

fn ids() -> (NodeId, NodeId) {
    (id("m5"), id("m1"))
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
            MoveEvent::Final(Ok(Vec::new())),
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
    assert_eq!(actions, vec![MoveAction::FinalSnapshot]);

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
    assert_eq!(actions, vec![MoveAction::FinalSnapshot]);

    let (nothing, actions) = next(
        &frozen,
        MoveEvent::MoveNow {
            working: Vec::new(),
        },
        true,
    );
    assert_eq!(nothing.step, MoveStep::FinalSnapshot);
    assert!(nothing.cut.is_empty());
    assert_eq!(actions, vec![MoveAction::FinalSnapshot]);

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
        MoveEvent::Final(Ok(vec![("c".to_string(), "repo changed".to_string())])),
        true,
    );
    assert_eq!(saga.moving, vec!["a".to_string()]);
    assert_eq!(saga.staying.len(), 2);
    assert!(saga
        .staying
        .contains(&("c".to_string(), "repo changed".to_string())));
    assert_eq!(saga.step, MoveStep::StopOld);
    assert_eq!(
        actions,
        vec![MoveAction::StopOld {
            sessions: vec!["a".to_string()]
        }]
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
            MoveEvent::Final(Ok(Vec::new())),
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
                sessions: vec!["a".to_string()]
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
        .any(|a| matches!(a, MoveAction::Restart { sessions } if sessions.len() == 2)));
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
            MoveEvent::Final(Ok(Vec::new())),
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
    assert_eq!(
        actions,
        vec![MoveAction::Switch {
            term: 4,
            attempt: 0
        }]
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
    assert!(matches!(actions.as_slice(), [MoveAction::Resume { .. }]));

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
    assert!(actions.is_empty());
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
            | MoveAction::FinalSnapshot
            | MoveAction::StopOld { .. }
            | MoveAction::Switch { .. }
            | MoveAction::Announce
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
