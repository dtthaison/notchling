#![cfg(unix)]

mod common;

use common::{connect, notchling, scratch, Daemon};
use notchling_local_api::{ClientError, ClientKind};
use serde_json::{json, Value};

fn only_json(text: &str) -> Value {
    assert_eq!(text.trim_end().lines().count(), 1, "one value only: {text}");
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn error_of(out: &str) -> Value {
    only_json(out)["error"].clone()
}

#[test]
fn help_needs_no_daemon_and_says_every_command() {
    let dir = scratch("contract-help");
    let home = common::home(&dir.0);
    let root = dir.0.join("root");
    let (code, out, err) = notchling(&root, &home, &["help", "--json"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.is_empty(), "{err}");
    let help = only_json(&out);
    assert_eq!(help["api"], notchling_local_api::API_VERSION);
    let commands: Vec<&str> = help["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["command"].as_str())
        .collect();
    for want in ["settings set", "secret-request", "ask", "status"] {
        assert!(commands.contains(&want), "{want} in {commands:?}");
    }
    let codes: Vec<i64> = help["exitCodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["code"].as_i64())
        .collect();
    assert_eq!(codes, [0, 1, 2, 3, 4, 5, 6, 7, 124, 125]);

    let (code, text, _) = notchling(&root, &home, &["help"]);
    assert_eq!(code, 0);
    assert!(text.contains("notchling settings set"), "{text}");
}

#[test]
fn with_no_daemon_every_command_exits_3() {
    let dir = scratch("contract-off");
    let home = common::home(&dir.0);
    let root = dir.0.join("root");
    let commands: [&[&str]; 6] = [
        &["status"],
        &["sessions"],
        &["capabilities"],
        &["settings", "list"],
        &["settings", "set", "chatHistory", "8mb"],
        &["secrets"],
    ];
    for args in commands {
        let mut with_json = args.to_vec();
        with_json.push("--json");
        let (code, out, err) = notchling(&root, &home, &with_json);
        assert_eq!(code, 3, "{args:?}: {out} {err}");
        assert!(
            err.is_empty(),
            "{args:?}: nothing on stderr with --json: {err}"
        );
        let error = error_of(&out);
        assert_eq!(error["exit"], 3, "{args:?}");
        assert_eq!(error["code"], "not_running", "{args:?}");
        assert_eq!(error["reason"], "not_running", "{args:?}");
        assert_eq!(error["never"], false);
        assert!(error["request"].is_null());

        let (code, out, err) = notchling(&root, &home, args);
        assert_eq!(code, 3, "{args:?}: {err}");
        assert!(out.is_empty(), "{args:?}: {out}");
        assert!(err.trim_end().ends_with("(not_running)"), "{args:?}: {err}");
    }
}

#[test]
fn a_wrong_command_line_exits_2_and_changes_nothing() {
    let dir = scratch("contract-usage");
    let home = common::home(&dir.0);
    let root = dir.0.join("root");
    for args in [
        &["settings", "set", "chatHistory"][..],
        &["frobnicate"][..],
        &["settings"][..],
    ] {
        let mut with_json = args.to_vec();
        with_json.push("--json");
        let (code, out, err) = notchling(&root, &home, &with_json);
        assert_eq!(code, 2, "{args:?}: {out} {err}");
        assert!(err.is_empty(), "{args:?}: {err}");
        let error = error_of(&out);
        assert_eq!(
            (error["exit"].as_i64(), error["reason"].as_str()),
            (Some(2), Some("usage"))
        );
        assert_eq!(error["code"], "usage");
        assert!(!error["message"].as_str().unwrap().contains("frobnicate"));

        let (code, out, err) = notchling(&root, &home, args);
        assert_eq!(code, 2, "{args:?}");
        assert!(out.is_empty(), "{out}");
        assert!(err.trim_end().ends_with("(bad_request)"), "{args:?}: {err}");
    }
    assert!(!root.exists() || !root.join("notchling/settings.json").exists());
}

#[test]
fn a_secret_stops_with_125_and_one_json_line_on_stderr() {
    let dir = scratch("contract-secret");
    let home = common::home(&dir.0);
    let root = dir.0.join("root");
    let (code, out, err) = notchling(&root, &home, &["secret", "--json", "gh", "--", "true"]);
    assert_eq!(code, 125, "{out} {err}");
    assert!(out.is_empty(), "{out}");
    let error = only_json(&err)["error"].clone();
    assert_eq!(error["exit"], 125);
    assert_eq!(error["code"], "stopped");

    let (code, out, err) = notchling(&root, &home, &["secret", "gh", "--", "true"]);
    assert_eq!(code, 125, "{out} {err}");
    assert!(out.is_empty(), "{out}");
    assert!(err.trim_end().ends_with("(stopped)"), "{err}");
}

#[test]
fn a_running_daemon_answers_capabilities_for_a_shell() {
    let dir = scratch("contract-caps");
    let home = common::home(&dir.0);
    let root = dir.0.join("root");
    let _daemon = Daemon::start(&root, &home);
    let (code, out, err) = notchling(&root, &home, &["capabilities", "--json"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.is_empty(), "{err}");
    let caps = only_json(&out);
    assert_eq!(caps["caller"]["tier"], "agent");
    assert_eq!(caps["caller"]["tool"], "shell");
    assert_eq!(caps["caller"]["verified"], false);
    assert_eq!(caps["switches"]["acting"], false);
    assert_eq!(caps["app"]["attached"], false);
    let now = |op: &str| {
        caps["ops"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["op"] == op)
            .unwrap_or_else(|| panic!("{op} not listed"))["now"]
            .clone()
    };
    assert_eq!(now("daemon.capabilities"), "open");
    assert_eq!(now("session.permission"), "never");
    assert_eq!(now("session.new"), "not_yet");
    assert_eq!(now("git.status"), "app_only");
    assert_eq!(now("vault.find"), "needs_switch");

    let (code, text, _) = notchling(&root, &home, &["capabilities"]);
    assert_eq!(code, 0);
    assert!(!text.trim().is_empty());
}

#[test]
fn a_refusal_exits_4_or_2_with_its_reason_in_the_json_error() {
    let dir = scratch("contract-refused");
    let home = common::home(&dir.0);
    let root = dir.0.join("root");
    let _daemon = Daemon::start(&root, &home);
    let cases: [(&[&str], i32, &str, &str); 4] = [
        (
            &["settings", "set", "switch.acting", "on"],
            4,
            "never",
            "forbidden",
        ),
        (
            &["settings", "set", "noSuchKey", "x"],
            2,
            "unknown_key",
            "bad_request",
        ),
        (
            &["settings", "set", "showPet", "maybe"],
            2,
            "bad_value",
            "bad_request",
        ),
        (
            &["settings", "set", "limitMode", "off"],
            4,
            "no_app",
            "needs_approval",
        ),
    ];
    for (args, exit, reason, code_name) in cases {
        let mut with_json = args.to_vec();
        with_json.push("--json");
        let (code, out, err) = notchling(&root, &home, &with_json);
        assert_eq!(code, exit, "{args:?}: {out} {err}");
        assert!(err.is_empty(), "{args:?}: {err}");
        let error = error_of(&out);
        assert_eq!(error["exit"], exit, "{args:?}");
        assert_eq!(error["reason"], reason, "{args:?}");
        assert_eq!(error["code"], code_name, "{args:?}");
        assert_eq!(error["op"], "settings.set");
        assert_eq!(error["never"], reason == "never", "{args:?}");

        let (code, out, err) = notchling(&root, &home, args);
        assert_eq!(code, exit, "{args:?}");
        assert!(out.is_empty(), "{out}");
        let name = if exit == 4 {
            "(refused)"
        } else {
            "(bad_request)"
        };
        assert!(err.trim_end().ends_with(name), "{args:?}: {err}");
    }
    assert!(
        !root.join("notchling/settings.json").exists()
            || !std::fs::read_to_string(root.join("notchling/settings.json"))
                .unwrap()
                .contains("limitMode")
    );
}

#[test]
fn an_op_an_agent_may_not_call_says_why_on_the_wire() {
    let dir = scratch("contract-wire");
    let home = common::home(&dir.0);
    let root = dir.0.join("root");
    let _daemon = Daemon::start(&root, &home);
    let mut client = connect(&root, ClientKind::Cli);
    let data_of =
        |client: &mut notchling_local_api::Client, op: &str| match client.call(op, json!({})) {
            Err(ClientError::Api(e)) => e.data.unwrap_or(Value::Null),
            other => panic!("{op}: {other:?}"),
        };
    assert_eq!(
        data_of(&mut client, "session.permission")["reason"],
        "never"
    );
    assert_eq!(data_of(&mut client, "session.permission")["never"], true);
    let planned = data_of(&mut client, "session.new");
    assert_eq!(
        (planned["reason"].as_str(), planned["phase"].as_u64()),
        (Some("not_yet"), Some(4))
    );
    assert_eq!(data_of(&mut client, "git.status")["reason"], "app_only");
}
