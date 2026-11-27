use notchling_engine::versions::Tool;
use serde_json::{json, Value};

use super::client_check::{check_line, go_back, soak_line, update_button};
use super::client_model::{client_part_with, Extras};

const AT: i64 = 1_700_000_000_000;

fn row(installed: &str, latest: Option<&str>, method: &str) -> Value {
    json!({
        "tool": "claude",
        "installed": {"version": installed, "sha256": "ab"},
        "method": method,
        "path": "/Users/me/.local/bin/claude",
        "reference": null,
        "usable": false,
        "failed": [],
        "check": null,
        "soak": null,
        "latest": latest,
        "goBack": null,
    })
}

#[test]
fn update_shows_only_when_newer_and_possible() {
    let newer = row("2.1.324", Some("2.1.325"), "native");
    assert!(update_button(Tool::Claude, &newer, true).is_some());
    let same = row("2.1.324", Some("2.1.324"), "native");
    assert_eq!(update_button(Tool::Claude, &same, true), None);
    let older = row("2.1.324", Some("2.1.323"), "native");
    assert_eq!(update_button(Tool::Claude, &older, true), None);
    let unknown = row("2.1.324", Some("2.1.325"), "unknown");
    assert_eq!(update_button(Tool::Claude, &unknown, true), None);
    let none = row("2.1.324", None, "native");
    assert_eq!(update_button(Tool::Claude, &none, true), None);
    assert_eq!(update_button(Tool::Notchling, &newer, true), None);
}

#[test]
fn install_off_disables_update_with_a_note() {
    let newer = row("2.1.324", Some("2.1.325"), "native");
    let off = update_button(Tool::Claude, &newer, false).unwrap();
    assert!(!off.enabled);
    assert_eq!(
        off.note.as_deref(),
        Some(notchling_locale::settings::client_install_first())
    );
    let on = update_button(Tool::Claude, &newer, true).unwrap();
    assert!(on.enabled);
    assert_eq!(on.note, None);
}

#[test]
fn check_line_says_partly_and_what_wasnt_checked() {
    let mut r = row("2.1.324", None, "native");
    assert_eq!(
        check_line(Tool::Claude, &r, 0).text,
        notchling_locale::settings::client_not_checked()
    );
    r["reference"] = json!({
        "version": "2.1.324", "sha256": "ab", "result": "partly", "partly": true,
        "at": AT, "unchecked": ["screens", "dialogs", "turns"],
    });
    r["usable"] = json!(true);
    let line = check_line(Tool::Claude, &r, 0);
    assert_eq!(
        line.text,
        "Partly checked 22:13 · version, doctor, transcripts"
    );
    assert!(!line.failed);
    assert_eq!(
        line.details.as_deref(),
        Some("Not checked: screens, dialogs and turns")
    );
    let shifted = check_line(Tool::Claude, &r, 3600);
    assert_eq!(
        shifted.text,
        "Partly checked 23:13 · version, doctor, transcripts"
    );
    r["usable"] = json!(false);
    assert_eq!(
        check_line(Tool::Claude, &r, 0).text,
        notchling_locale::settings::client_not_checked()
    );
}

#[test]
fn failed_check_line_is_red_with_details() {
    let mut r = row("2.1.325", None, "native");
    r["reference"] = json!({
        "version": "2.1.324", "sha256": "ab", "result": "partly", "partly": true,
        "at": AT, "unchecked": [],
    });
    r["failed"] = json!([{
        "version": "2.1.325", "part": "files_flags", "at": AT,
        "why": "Codex no longer lists --json.",
    }]);
    let line = check_line(Tool::Claude, &r, 0);
    assert!(line.failed);
    assert_eq!(
        line.text,
        "Failed 22:13: Codex no longer lists --json. Workers stay on 2.1.324."
    );
    r["check"] = json!({"running": true});
    assert_eq!(check_line(Tool::Claude, &r, 0).text, "Checking…");
}

#[test]
fn soak_line_red_when_unknown() {
    let mut r = row("2.1.324", None, "native");
    assert_eq!(soak_line(&r), None);
    r["soak"] =
        json!({"since": "2.1.324", "turns": 40, "unknown": 0, "line": "40 turns, none unknown"});
    assert_eq!(
        soak_line(&r),
        Some(("40 turns, none unknown".to_owned(), false))
    );
    r["soak"] =
        json!({"since": "2.1.324", "turns": 40, "unknown": 2, "line": "40 turns, 2 unknown"});
    assert_eq!(
        soak_line(&r),
        Some(("40 turns, 2 unknown".to_owned(), true))
    );
}

#[test]
fn go_back_only_with_an_exact_method_and_a_previous() {
    let mut r = row("2.1.325", None, "native");
    assert_eq!(go_back(Tool::Claude, &r), None);
    r["goBack"] = json!("2.1.323");
    r["reference"] = json!({"version": "2.1.323", "at": AT, "partly": true, "unchecked": []});
    let back = go_back(Tool::Claude, &r).unwrap();
    assert_eq!(back.to, "2.1.323");
    assert_eq!(back.label, "Go Back to 2.1.323");
    assert_eq!(
        back.confirm,
        "Claude Code will update itself again at its next check. Workers stay on 2.1.323."
    );
}

#[test]
fn the_part_carries_the_pieces_and_the_switch() {
    let mut r = row("2.1.324", Some("2.1.325"), "native");
    r["goBack"] = json!("2.1.323");
    let get = json!({"tools": [r]});
    let off = client_part_with(
        Tool::Claude,
        &get,
        &Value::Null,
        "/Users/me",
        Extras::default(),
    );
    assert_eq!(off.latest.as_deref(), Some("Latest 2.1.325"));
    assert!(!off.update.as_ref().unwrap().enabled);
    assert!(off.go_back.is_some());
    let on = client_part_with(
        Tool::Claude,
        &get,
        &Value::Null,
        "/Users/me",
        Extras {
            install_on: true,
            offset_secs: 0,
        },
    );
    assert!(on.update.as_ref().unwrap().enabled);
}
