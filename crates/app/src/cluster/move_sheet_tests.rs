use notchling_local_api::ops;
use serde_json::json;

use super::model::ClusterModel;
use super::move_sheet::{Button, MoveSheet, MoveStatus, Phase, Step, StepState, RUN};
use super::renders::{moving, names, status, ME, STANDBY};

fn status_at(step: &str) -> MoveStatus {
    MoveStatus::of(&moving(step)).expect("a move in the status")
}

fn sheet() -> MoveSheet {
    MoveSheet::running(STANDBY, "mac-mini")
}

#[test]
fn move_sheet_lists_moving_and_staying_sessions() {
    let view = sheet().view(Some(&status_at("freeze")), &names());
    let moving: Vec<&str> = view.moving.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        moving,
        [
            "Fix the parser",
            "Write the docs",
            "Run the full test suite"
        ]
    );
    assert!(view.moving.iter().all(|s| s.note.is_none()));
    let staying: Vec<(&str, Option<&str>)> = view
        .staying
        .iter()
        .map(|s| (s.name.as_str(), s.note.as_deref()))
        .collect();
    assert_eq!(
        staying,
        [
            (
                "Benchmark the sync",
                Some("Its folder has changes that aren't pushed")
            ),
            ("Try the fork", Some("Its branch isn't pushed yet")),
        ]
    );
}

#[test]
fn a_session_without_a_known_name_shows_the_start_of_its_id() {
    let view = sheet().view(Some(&status_at("freeze")), &Default::default());
    assert_eq!(view.moving[0].name, "j-parser");
    assert_eq!(view.moving[1].name, "codex:c-");
}

#[test]
fn steps_run_from_prepare_to_done_with_a_line_each() {
    let view = sheet().view(Some(&status_at("final_snapshot")), &names());
    assert_eq!(view.steps.len(), RUN.len());
    let states: Vec<StepState> = view.steps.iter().map(|s| s.state).collect();
    assert_eq!(
        states,
        [
            StepState::Done,
            StepState::Done,
            StepState::Done,
            StepState::Current,
            StepState::Waiting,
            StepState::Waiting,
            StepState::Waiting,
            StepState::Waiting,
            StepState::Waiting,
        ]
    );
    assert_eq!(view.steps[0].text, "Checking mac-mini");
    assert_eq!(
        view.steps[2].text,
        "Waiting for the sessions to finish their turns"
    );
    assert_eq!(view.steps[8].text, "mac-mini is the primary now");

    let done = sheet().view(Some(&status_at("done")), &names());
    assert!(done.steps.iter().all(|s| s.state == StepState::Done));
}

#[test]
fn move_sheet_offers_move_now_while_freezing() {
    let at = |step: &str| sheet().view(Some(&status_at(step)), &names()).buttons;
    assert_eq!(at("freeze"), [Button::MoveNow, Button::Close]);
    for step in [
        "prepare",
        "catch_up",
        "final_snapshot",
        "stop_old",
        "switch",
        "resume",
        "announce",
        "done",
        "undone",
    ] {
        assert!(!at(step).contains(&Button::MoveNow), "{step}");
    }
    let model = ClusterModel::from_status(&moving("freeze"), ME);
    let ask = model.move_now().expect("a running move");
    assert_eq!(ask.op, ops::CLUSTER_MOVE_NOW);
    assert_eq!(ask.args, json!({}));
    assert!(ClusterModel::from_status(&moving("done"), ME)
        .move_now()
        .is_none());
}

#[test]
fn the_sessions_that_were_cut_are_named() {
    let view = sheet().view(Some(&status_at("freeze")), &names());
    assert!(
        view.notes
            .iter()
            .any(|n| n.text.contains("Run the full test suite") && !n.problem),
        "{:?}",
        view.notes
    );
    let quiet = sheet().view(Some(&status_at("catch_up")), &names());
    assert!(quiet.notes.is_empty());
}

#[test]
fn move_sheet_offers_cancel_only_before_switch() {
    let ask = MoveSheet::new(STANDBY, "mac-mini");
    assert_eq!(ask.phase, Phase::Confirm);
    assert_eq!(
        ask.view(None, &names()).buttons,
        [Button::Start, Button::Cancel]
    );
    for name in [
        "prepare",
        "catch_up",
        "freeze",
        "final_snapshot",
        "stop_old",
        "switch",
        "resume",
        "announce",
        "done",
        "undone",
    ] {
        let buttons = sheet().view(Some(&status_at(name)), &names()).buttons;
        assert!(!buttons.contains(&Button::Cancel), "{name}");
        assert!(!buttons.contains(&Button::Start), "{name}");
        assert!(buttons.contains(&Button::Close), "{name}");
    }
    let starting = MoveSheet {
        phase: Phase::Starting,
        ..MoveSheet::new(STANDBY, "mac-mini")
    };
    assert!(!starting
        .view(None, &names())
        .buttons
        .contains(&Button::Cancel));
    let refused = MoveSheet {
        phase: Phase::Refused("nope".into()),
        ..MoveSheet::new(STANDBY, "mac-mini")
    };
    assert_eq!(refused.view(None, &names()).buttons, [Button::Close]);
}

#[test]
fn the_resume_proof_refusal_is_shown_plainly() {
    let refused = MoveSheet {
        phase: Phase::Refused("Moving the primary needs the resume proof first".into()),
        ..MoveSheet::new(STANDBY, "mac-mini")
    };
    let view = refused.view(None, &names());
    assert_eq!(view.notes.len(), 1);
    assert_eq!(
        view.notes[0].text,
        "Moving the primary needs the resume proof first"
    );
    assert!(view.notes[0].problem);
    assert!(view.steps.is_empty());
}

#[test]
fn a_move_that_was_undone_says_why_and_lists_no_steps() {
    let mut status = moving("undone");
    status["move"]["reason"] = json!("mac-mini went offline");
    let view = sheet().view(MoveStatus::of(&status).as_ref(), &names());
    assert!(view.steps.is_empty());
    assert_eq!(view.notes.len(), 1);
    assert_eq!(
        view.notes[0].text,
        "Couldn't move the primary: mac-mini went offline"
    );
    assert!(view.notes[0].problem);
}

#[test]
fn a_session_that_failed_to_start_is_named_with_why() {
    let mut status = moving("done");
    status["move"]["failed"] = json!([["j-parser", "the folder isn't trusted"]]);
    let view = sheet().view(MoveStatus::of(&status).as_ref(), &names());
    assert!(view.notes.iter().any(|n| n.problem
        && n.text.contains("Fix the parser")
        && n.text.contains("isn't trusted")));
}

#[test]
fn status_names_read_as_steps() {
    for (name, step) in [
        ("prepare", Step::Prepare),
        ("catch_up", Step::CatchUp),
        ("freeze", Step::Freeze),
        ("final_snapshot", Step::FinalSnapshot),
        ("stop_old", Step::StopOld),
        ("switch", Step::Switch),
        ("resume", Step::Resume),
        ("announce", Step::Announce),
        ("done", Step::Done),
        ("undone", Step::Undone),
    ] {
        assert_eq!(Step::from_name(name), Some(step));
    }
    assert_eq!(Step::from_name("Undo"), None);
    assert!(MoveStatus::of(&status()).is_none());
    assert!(MoveStatus::of(&json!({"move": null})).is_none());
}

#[test]
fn a_finished_move_is_not_running() {
    assert!(status_at("freeze").running());
    assert!(!status_at("done").running());
    assert!(!status_at("undone").running());
}
