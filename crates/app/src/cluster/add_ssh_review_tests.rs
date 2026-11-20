use std::sync::atomic::Ordering;

use notchling_engine::machines::registry::Machine;

use super::add_rig_tests::Rig;
use super::add_sheet::{machine_view, Button, Draft};
use super::add_test_run::{may_test, test_signature, TestRun};
use super::machine_add::{MachineAdd, Phase};

const ME: &str = "sondoan";

fn draft(address: &str) -> Draft {
    Draft {
        address: address.into(),
        ..Draft::default()
    }
}

#[test]
fn no_key_in_the_command_leaves_the_machine_without_one() {
    let rig = Rig::new(vec![]);
    let mut add = MachineAdd::new();
    let probe = add
        .begin(&rig.parts(), &draft("ssh deploy@app-1.example"), None)
        .expect("begins");
    let answer = probe.run(&rig.engine);
    add.answered(&rig.parts(), answer);
    assert_eq!(rig.saved("app-1").expect("saved").ssh_key(), None);
}

#[test]
fn editing_stops_a_running_test_and_ignores_its_late_answer() {
    let mut run = TestRun::default();
    let stop = run.start("a".into(), 1);
    assert!(run.is_running());
    assert!(!stop.load(Ordering::SeqCst));
    run.reset();
    assert!(stop.load(Ordering::SeqCst));
    assert!(!run.is_running());
    assert!(!run.is_stopped());
    run.finish(1, "a", Vec::new());
    assert!(!run.is_running());
    let again = run.start("b".into(), 2);
    assert!(!again.load(Ordering::SeqCst));
    run.finish(2, "c", Vec::new());
    assert!(!run.is_running());
    assert!(run.shown("c").is_empty());
}

#[test]
fn a_bad_extra_row_is_shown_and_disables_test_and_add() {
    use super::addresses_form::RowDraft;
    let mut d = draft("ssh deploy@10.1.2.3");
    d.more = vec![RowDraft {
        text: "bad host!".into(),
        ..RowDraft::default()
    }];
    let shown = machine_view(&Phase::Form, &d, ME, &[]);
    assert!(shown.target.is_none());
    assert!(shown.error.is_some());
    assert!(!shown.allows(Button::Add));
    assert!(!may_test(shown.target.is_some(), false, 0, 4));
}

#[test]
fn the_hidden_first_row_is_never_swapped() {
    use super::addresses_form::may_swap;
    assert!(!may_swap(3, 1, 0, true));
    assert!(!may_swap(3, 0, 1, true));
    assert!(may_swap(3, 1, 2, true));
    assert!(may_swap(3, 1, 0, false));
    assert!(!may_swap(3, 1, 3, false));
    assert!(!may_swap(3, 1, 1, false));
}

#[test]
fn a_full_set_of_lanes_disables_test() {
    assert!(may_test(true, false, 3, 4));
    assert!(!may_test(true, false, 4, 4));
    assert!(!may_test(true, true, 0, 4));
    assert!(!may_test(false, false, 0, 4));
}

#[test]
fn the_test_signature_ignores_the_name_and_follows_the_jump_machine() {
    let mut bastion = Machine::new("oracle", "oracle.example", "ops");
    let mut one = draft("ssh -J oracle deploy@10.1.2.3");
    let sig = |d: &Draft, m: &Machine| test_signature(d, std::slice::from_ref(m), Some("oracle"));
    let base = sig(&one, &bastion);
    one.name = "renamed".into();
    assert_eq!(sig(&one, &bastion), base);
    bastion.host = "elsewhere.example".into();
    bastion.set_ssh_key(Some("~/.ssh/other"));
    assert_ne!(sig(&one, &bastion), base);
    let two = draft("ssh -J oracle deploy@10.1.2.4");
    assert_ne!(sig(&two, &bastion), sig(&one, &bastion));
}

#[test]
fn a_jump_with_a_user_or_port_is_refused_in_the_sheet() {
    let known = vec!["oracle".to_owned()];
    let shown = machine_view(
        &Phase::Form,
        &draft("ssh -J me@oracle deploy@10.1.2.3"),
        ME,
        &known,
    );
    assert!(shown
        .error
        .as_deref()
        .is_some_and(|e| e.contains("already in Machines")));
    assert!(!shown.allows(Button::Add));
}
