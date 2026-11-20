use crate::acting::ACTING_OFF;
use crate::acting_ops_tests::{app, caller, code, sessions, setup, Setup, PROMPT};
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Value};

const SWITCH: &str = include_str!("../../engine/tests/fixtures/dialog-made-up/switch-model.txt");

fn dialog_screen() -> Vec<String> {
    SWITCH.lines().map(str::to_owned).collect()
}

fn showing(s: &Setup) {
    *s.dialog.lock().unwrap() = Some((dialog_screen(), true));
}

fn pick(number: u64, label: &str) -> Value {
    json!({"id": "job-a", "title": "Switch model?", "number": number, "label": label})
}

fn other_caller() -> Caller {
    Caller {
        tier: Tier::Agent,
        ..caller(ClientKind::Cli)
    }
}

#[test]
fn the_app_reads_the_dialog_the_terminal_shows_and_types_nothing() {
    let s = setup(true, PROMPT, sessions());
    showing(&s);
    let read = s
        .ops
        .call(&app(), ops::SESSION_DIALOG, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(
        read,
        json!({"dialog": {
            "title": "Switch model?",
            "body": [
                "Your next response will be slower and use more tokens",
                "This conversation is cached for the current model. Switching to Opus 5.5 means the full history gets re-read."
            ],
            "options": [
                {"number": 1, "label": "Yes, switch to Opus 5.5", "selected": true},
                {"number": 2, "label": "No, go back", "selected": false},
            ],
        }})
    );
    assert!(s.writes.lock().unwrap().is_empty(), "nothing typed");
}

#[test]
fn a_screen_without_a_dialog_reads_as_none() {
    let s = setup(true, PROMPT, sessions());
    let read = s
        .ops
        .call(&app(), ops::SESSION_DIALOG, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(read, json!({"dialog": null}));
}

#[test]
fn reading_waits_for_the_switch_and_the_app() {
    let off = setup(false, PROMPT, sessions());
    let read = off
        .ops
        .call(&app(), ops::SESSION_DIALOG, &json!({"id": "job-a"}));
    assert_eq!(code(read), (ErrorCode::Forbidden, ACTING_OFF.to_string()));
    let on = setup(true, PROMPT, sessions());
    let read = on.ops.call(
        &other_caller(),
        ops::SESSION_DIALOG,
        &json!({"id": "job-a"}),
    );
    assert_eq!(code(read).0, ErrorCode::Forbidden);
    assert!(on.attached.lock().unwrap().is_empty());
    assert!(off.attached.lock().unwrap().is_empty());
}

#[test]
fn a_session_without_a_terminal_is_not_read() {
    let s = setup(true, PROMPT, sessions());
    let read = s
        .ops
        .call(&app(), ops::SESSION_DIALOG, &json!({"id": "term"}));
    assert!(read.is_err());
    assert!(s.attached.lock().unwrap().is_empty());
}

#[test]
fn picking_an_option_types_only_its_number() {
    let s = setup(true, PROMPT, sessions());
    showing(&s);
    let done = s
        .ops
        .call(&app(), ops::SESSION_DIALOG_PICK, &pick(2, "No, go back"))
        .unwrap();
    assert_eq!(done, json!({"answered": "terminal", "number": 2}));
    assert_eq!(s.writes.lock().unwrap().as_slice(), ["2"]);
}

#[test]
fn nothing_is_typed_when_the_dialog_is_not_the_one_the_user_saw() {
    let s = setup(true, PROMPT, sessions());
    showing(&s);
    for wrong in [
        json!({"id": "job-a", "title": "Delete everything?", "number": 1, "label": "Yes, switch to Opus 5.5"}),
        pick(1, "Yes, delete it"),
        pick(3, "Maybe"),
    ] {
        let result = s.ops.call(&app(), ops::SESSION_DIALOG_PICK, &wrong);
        assert!(result.is_err(), "{wrong}");
    }
    assert!(s.writes.lock().unwrap().is_empty(), "nothing typed");
}

#[test]
fn nothing_is_typed_when_no_dialog_shows() {
    let s = setup(true, PROMPT, sessions());
    let result = s.ops.call(
        &app(),
        ops::SESSION_DIALOG_PICK,
        &pick(1, "Yes, switch to Opus 5.5"),
    );
    assert!(result.is_err());
    assert!(s.writes.lock().unwrap().is_empty(), "nothing typed");
}

#[test]
fn picking_is_refused_while_the_switch_is_off_and_for_other_callers() {
    let off = setup(false, PROMPT, sessions());
    let result = off.ops.call(
        &app(),
        ops::SESSION_DIALOG_PICK,
        &pick(1, "Yes, switch to Opus 5.5"),
    );
    assert_eq!(code(result), (ErrorCode::Forbidden, ACTING_OFF.to_string()));
    let on = setup(true, PROMPT, sessions());
    showing(&on);
    let result = on.ops.call(
        &other_caller(),
        ops::SESSION_DIALOG_PICK,
        &pick(1, "Yes, switch to Opus 5.5"),
    );
    assert_eq!(code(result).0, ErrorCode::Forbidden);
    assert!(on.writes.lock().unwrap().is_empty());
}

const UNREADABLE: [&str; 3] = [
    "────────────────────────────────────────",
    " Something this parser does not know",
    " Enter to confirm · Esc to cancel",
];

#[test]
fn dismissing_sends_esc_while_a_dialog_is_open_whatever_the_session_says() {
    for id in ["job-a", "job-idle"] {
        let s = setup(true, PROMPT, sessions());
        showing(&s);
        let done = s
            .ops
            .call(&app(), ops::SESSION_DIALOG_DISMISS, &json!({ "id": id }))
            .unwrap();
        assert_eq!(done, json!({"dismissed": true}), "{id}");
        assert_eq!(s.writes.lock().unwrap().as_slice(), ["\x1b"], "{id}");
    }
}

#[test]
fn dismissing_also_closes_a_dialog_the_parser_cannot_read() {
    let s = setup(true, PROMPT, sessions());
    *s.dialog.lock().unwrap() = Some((UNREADABLE.iter().map(|l| l.to_string()).collect(), true));
    s.ops
        .call(&app(), ops::SESSION_DIALOG_DISMISS, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(s.writes.lock().unwrap().as_slice(), ["\x1b"]);
}

#[test]
fn dismissing_sends_no_esc_when_no_dialog_is_open() {
    let s = setup(true, PROMPT, sessions());
    for id in ["job-a", "job-idle"] {
        let result = s
            .ops
            .call(&app(), ops::SESSION_DIALOG_DISMISS, &json!({ "id": id }));
        let (_, message) = code(result);
        assert!(message.contains("isn't open"), "{id}: {message}");
    }
    assert!(s.writes.lock().unwrap().is_empty(), "no Esc typed");
}

#[test]
fn dismissing_waits_for_the_switch_and_the_app() {
    let off = setup(false, PROMPT, sessions());
    showing(&off);
    let result = off
        .ops
        .call(&app(), ops::SESSION_DIALOG_DISMISS, &json!({"id": "job-a"}));
    assert_eq!(code(result), (ErrorCode::Forbidden, ACTING_OFF.to_string()));
    let on = setup(true, PROMPT, sessions());
    showing(&on);
    let result = on.ops.call(
        &other_caller(),
        ops::SESSION_DIALOG_DISMISS,
        &json!({"id": "job-a"}),
    );
    assert_eq!(code(result).0, ErrorCode::Forbidden);
    assert!(on.writes.lock().unwrap().is_empty());
    assert!(off.writes.lock().unwrap().is_empty());
}

#[test]
fn a_number_outside_one_to_nine_is_refused_before_the_terminal_is_touched() {
    let s = setup(true, PROMPT, sessions());
    showing(&s);
    for number in [0, 10, 99] {
        let result = s.ops.call(
            &app(),
            ops::SESSION_DIALOG_PICK,
            &pick(number, "Yes, switch to Opus 5.5"),
        );
        assert_eq!(code(result).0, ErrorCode::BadRequest, "{number}");
    }
    assert!(s.attached.lock().unwrap().is_empty(), "never attached");
    assert!(s.writes.lock().unwrap().is_empty());
}
