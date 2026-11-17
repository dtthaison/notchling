use crate::acting::ACTING_OFF;
use crate::acting_ops_tests::{app, caller, code, sessions, setup};
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, Tier};
use serde_json::json;

const RUNNING: &str = include_str!("../../engine/tests/fixtures/shell-live/running-lines.txt");

fn screen() -> Vec<&'static str> {
    RUNNING.lines().skip(1).collect()
}

fn ask() -> serde_json::Value {
    json!({"id": "job-a", "command": "npm run build"})
}

#[test]
fn the_app_reads_the_lines_of_a_command_that_is_still_running() {
    let s = setup(true, &screen(), sessions());
    let read = s
        .ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &ask())
        .unwrap();
    assert_eq!(
        read,
        json!({"lines": [
            "> app@1.0.0 build",
            "> tsc -p .",
            "",
            "src/main.ts(3,7): error TS2322: Type 'string' is not assignable to type 'number'.",
            "  3 | const n: number = \"x\";"
        ]})
    );
    assert_eq!(
        s.attached.lock().unwrap().as_slice(),
        [("abcd1234".to_string(), Some("/lanes/b".to_string()))]
    );
    assert!(s.writes.lock().unwrap().is_empty(), "nothing typed");
}

#[test]
fn it_waits_for_the_switch_and_the_app() {
    let off = setup(false, &screen(), sessions());
    let read = off.ops.call(&app(), ops::SESSION_SHELL_OUTPUT, &ask());
    assert_eq!(code(read), (ErrorCode::Forbidden, ACTING_OFF.to_string()));
    let on = setup(true, &screen(), sessions());
    let agent = Caller {
        tier: Tier::Agent,
        ..caller(ClientKind::Cli)
    };
    let read = on.ops.call(&agent, ops::SESSION_SHELL_OUTPUT, &ask());
    assert_eq!(code(read).0, ErrorCode::Forbidden);
    assert!(on.attached.lock().unwrap().is_empty());
    assert!(off.attached.lock().unwrap().is_empty());
}

#[test]
fn a_session_that_is_not_working_is_never_attached() {
    let s = setup(true, &screen(), sessions());
    let read = s
        .ops
        .call(
            &app(),
            ops::SESSION_SHELL_OUTPUT,
            &json!({"id": "job-idle", "command": "npm run build"}),
        )
        .unwrap();
    assert_eq!(read, json!({"lines": []}));
    assert!(
        s.attached.lock().unwrap().is_empty(),
        "an attach would wake a job that has ended"
    );
}

#[test]
fn a_session_without_a_terminal_and_a_missing_command_are_refused() {
    let s = setup(true, &screen(), sessions());
    let term = s.ops.call(
        &app(),
        ops::SESSION_SHELL_OUTPUT,
        &json!({"id": "term", "command": "ls"}),
    );
    assert_eq!(code(term).0, ErrorCode::BadRequest);
    let bare = s
        .ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &json!({"id": "job-a"}));
    assert_eq!(code(bare).0, ErrorCode::BadRequest);
    assert!(s.attached.lock().unwrap().is_empty());
}

#[test]
fn a_command_that_is_not_on_the_screen_has_no_lines() {
    let s = setup(true, &screen(), sessions());
    let read = s
        .ops
        .call(
            &app(),
            ops::SESSION_SHELL_OUTPUT,
            &json!({"id": "job-a", "command": "ls -la"}),
        )
        .unwrap();
    assert_eq!(read, json!({"lines": []}));
}
