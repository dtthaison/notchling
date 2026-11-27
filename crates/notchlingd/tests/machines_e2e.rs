#![cfg(unix)]

mod common;

use common::{connect, home, notchling, scratch, Daemon};
use notchling_local_api::{ClientError, ClientKind};
use serde_json::{json, Value};
use std::path::Path;

fn run(root: &Path, home: &Path, args: &[&str]) -> (i32, Value, String) {
    let (code, out, err) = notchling(root, home, args);
    let value = serde_json::from_str(&out).unwrap_or(Value::Null);
    (code, value, err)
}

fn reason(out: &Value) -> String {
    out["error"]["reason"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn needs_consent(err: &ClientError) -> bool {
    match err {
        ClientError::Api(e) => e
            .data
            .as_ref()
            .is_some_and(|d| d["reason"] == "needs_consent"),
        _ => false,
    }
}

#[test]
fn the_machines_switch_is_off_at_every_start_and_every_op_says_so() {
    let dir = scratch("machines-e2e-off");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start_for_app(&root, &home);
    let mut app = connect(&root, ClientKind::App);
    app.call(
        "settings.set_switch",
        json!({"switch": "machines", "on": true}),
    )
    .unwrap();
    app.call(
        "settings.set_switch",
        json!({"switch": "machines", "on": false}),
    )
    .unwrap();
    let switches = app.call("settings.switches", json!({})).unwrap();
    assert_eq!(switches["machines"], false, "{switches}");
    for args in [
        vec!["machines", "list", "--json"],
        vec!["machines", "peers", "--json"],
        vec!["network", "list", "--json"],
        vec!["machines", "add", "deploy@newbox.test", "--json"],
    ] {
        let (code, out, _) = run(&root, &home, &args);
        assert_ne!(code, 0, "{args:?} {out}");
        assert!(
            ["switch_off", "no_session"].contains(&reason(&out).as_str()),
            "{args:?} {out}"
        );
    }
}

#[test]
fn a_shell_is_no_session_once_the_switch_is_on_and_a_stray_switch_citation_is_refused() {
    let dir = scratch("machines-e2e-shell");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start_for_app(&root, &home);
    let mut app = connect(&root, ClientKind::App);
    app.call(
        "settings.set_switch",
        json!({"switch": "machines", "on": true}),
    )
    .unwrap();
    for args in [
        vec!["machines", "list", "--json"],
        vec!["network", "list", "--json"],
        vec!["machines", "add", "deploy@newbox.test", "--json"],
        vec!["network", "switch", "work.example", "--json"],
    ] {
        let (code, out, _) = run(&root, &home, &args);
        assert_ne!(code, 0, "{args:?} {out}");
        assert_eq!(reason(&out), "no_session", "{args:?} {out}");
    }
    let stray = app
        .call(
            "tailscale.switch",
            json!({"tailnet": "work.example", "consentId": "no-such-card"}),
        )
        .unwrap_err();
    assert!(needs_consent(&stray), "{stray:?}");
    let apply = app.call("machines.apply", json!({})).unwrap_err();
    assert!(needs_consent(&apply), "{apply:?}");
}
