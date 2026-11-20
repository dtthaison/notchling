use super::*;
use crate::outbox_courier::asked_window;
use crate::outbox_testkit::{rig, Rig, IDLE, SWITCH_MODEL, T0};
use notchling_engine::outbox::{State, Verdict, TURN_SETTLE_MS};

const MODEL: &str = "/model claude-opus-5-5";

fn shown(r: &Rig, id: &str) -> Option<(State, Option<&'static str>)> {
    r.row(id).map(|row| (row.state, row.why.map(Why::as_str)))
}

fn sent_model_command(r: &Rig, p: &mut Postman) -> (String, i64) {
    r.open_dialog_after_a_command(SWITCH_MODEL);
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let row = r.send("job-idle", MODEL);
    p.pass(r.at(T0));
    let typed_at = r.at(T0 + TURN_SETTLE_MS);
    p.pass(typed_at);
    (row.id, typed_at)
}

#[test]
fn a_model_command_that_opens_a_confirmation_is_delivered_and_needs_an_answer() {
    let r = rig();
    let mut p = r.postmen.postman(IDLE);
    let (id, _) = sent_model_command(&r, &mut p);
    assert_eq!(r.returns(), 1, "the command was typed and sent once");
    assert_eq!(shown(&r, &id), Some((State::Waiting, Some("answer"))));
}

#[test]
fn the_row_waits_for_the_choice_and_is_done_when_the_dialog_is_gone() {
    let r = rig();
    let mut p = r.postmen.postman(IDLE);
    let (id, typed_at) = sent_model_command(&r, &mut p);
    r.files.set_status(IDLE, "waiting");
    p.pass(r.at(typed_at + 10_000));
    assert_eq!(shown(&r, &id), Some((State::Waiting, Some("answer"))));
    assert_eq!(r.returns(), 1, "never typed again while it waits");
    r.files.set_status(IDLE, "idle");
    p.pass(r.at(typed_at + 12_000));
    assert_eq!(r.row(&id), None, "answered, so the row is done");
    assert_eq!(r.returns(), 1);
    let gone = r.room.named("outbox.removed");
    assert_eq!(gone.last().map(|g| g["why"].clone()), Some("sent".into()));
}

fn dialog_stays_up(r: &Rig) {
    *r.dialog.lock().unwrap() = Some((SWITCH_MODEL.iter().map(|l| l.to_string()).collect(), true));
}

#[test]
fn the_row_stays_while_the_dialog_is_still_on_screen_even_if_the_job_says_idle() {
    let r = rig();
    let mut p = r.postmen.postman(IDLE);
    let (id, typed_at) = sent_model_command(&r, &mut p);
    dialog_stays_up(&r);
    let behind = r.send("job-idle", "and then push");
    p.pass(r.at(typed_at + 20_000));
    assert_eq!(shown(&r, &id), Some((State::Waiting, Some("answer"))));
    assert_eq!(r.returns(), 1, "nothing typed into the open dialog");
    assert!(
        r.row(&behind.id)
            .is_some_and(|row| row.state != State::Sending),
        "the message behind it is held"
    );
    *r.dialog.lock().unwrap() = None;
    p.pass(r.at(typed_at + 22_000));
    assert_eq!(r.row(&id), None, "answered, so the row is done");
}

#[test]
fn a_model_command_gets_a_longer_wait_for_its_confirmation_than_other_commands() {
    let (model_quiet, model_wait) = asked_window("/model claude-opus-5-5");
    let (other_quiet, other_wait) = asked_window("/clear");
    assert!(model_quiet > other_quiet && model_wait > other_wait);
    assert_eq!(asked_window("  /model"), (model_quiet, model_wait));
    assert_eq!(asked_window("/modeling"), (other_quiet, other_wait));
    assert_eq!((other_quiet, other_wait), (400, 1_500));
    assert!(model_wait <= 5_000, "bounded");
}
