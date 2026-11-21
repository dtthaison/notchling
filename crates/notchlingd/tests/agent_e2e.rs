#![cfg(unix)]

mod common;

use common::{audit, home, notchling, scratch, Daemon};
use serde_json::Value;
use std::path::Path;

fn run(root: &Path, home: &Path, args: &[&str]) -> (i32, Value, String) {
    let (code, out, err) = notchling(root, home, args);
    let value = serde_json::from_str(&out).unwrap_or(Value::Null);
    (code, value, err)
}

fn request_of(out: &Value) -> &Value {
    &out["error"]["request"]
}

fn called(root: &Path, want: usize) -> Vec<(String, String)> {
    let lines = audit::wait_until(root, |all| {
        all.iter().filter(|l| l["kind"] == "agent.called").count() >= want
    });
    lines
        .iter()
        .filter(|l| l["kind"] == "agent.called")
        .map(|l| {
            (
                l["f"]["op"].as_str().unwrap_or_default().to_string(),
                l["f"]["result"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn consent_thens(root: &Path, want: usize) -> Vec<String> {
    let lines = audit::wait_until(root, |all| {
        all.iter().filter(|l| l["kind"] == "agent.consent").count() >= want
    });
    lines
        .iter()
        .filter(|l| l["kind"] == "agent.consent")
        .map(|l| l["f"]["then"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn an_agent_opens_checks_and_withdraws_a_request_with_no_app_attached() {
    let dir = scratch("agent-e2e");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start(&root, &home);

    let (code, out, err) = run(
        &root,
        &home,
        &["settings", "set", "limitMode", "off", "--no-wait", "--json"],
    );
    assert_eq!(code, 6, "{out} {err}");
    assert_eq!(out["error"]["code"], "pending");
    assert!(err.is_empty(), "{err}");
    let request = request_of(&out);
    let id = request["id"].as_str().unwrap().to_string();
    assert!(id.starts_with('c') && id.len() == 33, "{id}");
    assert_eq!(request["state"], "waiting");
    assert_eq!(request["kind"], "setting");

    let (code, out, err) = run(&root, &home, &["consent", &id, "--json"]);
    assert_eq!(code, 6, "{out} {err}");
    assert_eq!(request_of(&out)["id"], id.as_str());
    assert_eq!(request_of(&out)["state"], "waiting");

    let (code, out, _) = run(&root, &home, &["consents", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(
        out["requests"],
        serde_json::json!([]),
        "a caller in no verified session lists nothing"
    );

    let (code, out, err) = run(&root, &home, &["consent", "cancel", &id, "--json"]);
    assert_eq!(code, 0, "{out} {err}");
    assert_eq!(out["cancelled"], true);

    let (code, out, _) = run(&root, &home, &["consent", &id, "--json"]);
    assert_eq!(code, 5);
    assert_eq!(out["error"]["reason"], "cancelled");
    let (code, out, _) = run(&root, &home, &["consent", "cancel", &id, "--json"]);
    assert_eq!(code, 1, "a closed request can't be withdrawn again: {out}");

    let (code, out, _) = run(&root, &home, &["consent", "c0000", "--json"]);
    assert_eq!(code, 1, "{out}");

    let acting = called(&root, 3);
    assert_eq!(
        acting,
        [
            ("settings.set".to_string(), "pending".to_string()),
            ("consent.cancel".to_string(), "ok".to_string()),
            ("consent.cancel".to_string(), "ok".to_string()),
        ],
        "every acting call is recorded once, reads are not"
    );
    assert_eq!(consent_thens(&root, 2), ["asked", "cancelled"]);
    let text = serde_json::to_string(&audit::lines(&root)).unwrap();
    assert!(
        !text.contains(&id),
        "the audit holds a hashed ref, never the id"
    );
}

#[test]
fn a_second_request_from_the_same_caller_is_busy_until_the_first_ends() {
    let dir = scratch("agent-e2e-busy");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start(&root, &home);

    let (code, out, _) = run(
        &root,
        &home,
        &["settings", "set", "limitMode", "off", "--no-wait", "--json"],
    );
    assert_eq!(code, 6, "{out}");
    let first = request_of(&out)["id"].as_str().unwrap().to_string();

    let (code, out, err) = run(
        &root,
        &home,
        &[
            "settings",
            "set",
            "limitSwitchAt",
            "90",
            "--no-wait",
            "--json",
        ],
    );
    assert_eq!(code, 7, "{out} {err}");
    assert_eq!(out["error"]["code"], "busy");
    assert!(out["error"]["retryAfterMs"].as_u64().unwrap() >= 1000);

    let (code, _, _) = run(&root, &home, &["consent", "cancel", &first, "--json"]);
    assert_eq!(code, 0);
    let (code, out, _) = run(
        &root,
        &home,
        &[
            "settings",
            "set",
            "limitSwitchAt",
            "90",
            "--no-wait",
            "--json",
        ],
    );
    assert_eq!(code, 6, "{out}");

    let acting = called(&root, 4);
    let results: Vec<&str> = acting.iter().map(|(_, r)| r.as_str()).collect();
    assert_eq!(
        results,
        ["pending", "failed", "ok", "pending"],
        "{acting:?}"
    );
}

#[test]
fn a_secret_request_is_saved_and_never_turns_the_vault_on() {
    let dir = scratch("agent-e2e-secret");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start(&root, &home);

    let (_, before, _) = run(&root, &home, &["settings", "get", "switch.vault", "--json"]);
    assert_eq!(before["value"], false, "{before}");

    let (code, out, err) = run(
        &root,
        &home,
        &[
            "secret-request",
            "npm",
            "--url",
            "https://registry.npmjs.org",
            "--reason",
            "to publish",
            "--no-wait",
            "--json",
        ],
    );
    assert_eq!(code, 6, "{out} {err}");
    let request = request_of(&out);
    assert_eq!(request["kind"], "secret_request");
    assert_eq!(request["target"], "npm");
    assert_eq!(request["state"], "waiting");
    assert_eq!(request["answerOn"], "mac");
    let id = request["id"].as_str().unwrap().to_string();

    let (_, after, _) = run(&root, &home, &["settings", "get", "switch.vault", "--json"]);
    assert_eq!(
        after["value"], false,
        "the request leaves the Vault as it was"
    );
    let (code, out, _) = run(&root, &home, &["consent", &id, "--json"]);
    assert_eq!(code, 6, "{out}");
    assert!(
        !root.join("vault").exists() && !home.join("Library/Keychains").exists(),
        "no vault was created"
    );

    let (code, _, _) = run(&root, &home, &["consent", "cancel", &id, "--json"]);
    assert_eq!(code, 0);
    let acting = called(&root, 2);
    assert_eq!(
        acting,
        [
            ("vault.request".to_string(), "pending".to_string()),
            ("consent.cancel".to_string(), "ok".to_string()),
        ]
    );
    let text = serde_json::to_string(&audit::lines(&root)).unwrap();
    assert!(!text.contains("to publish"), "the reason is never recorded");
}

#[test]
fn an_unanswered_request_expires_at_its_end_and_the_agent_sees_exit_124() {
    let dir = scratch("agent-e2e-expire");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start(&root, &home);

    let (code, out, _) = run(
        &root,
        &home,
        &[
            "settings",
            "set",
            "limitMode",
            "off",
            "--no-wait",
            "--expires-in",
            "60",
            "--json",
        ],
    );
    assert_eq!(code, 6, "{out}");
    let id = request_of(&out)["id"].as_str().unwrap().to_string();

    let (code, out, _) = run(
        &root,
        &home,
        &["consent", &id, "--wait", "--timeout", "100", "--json"],
    );
    assert_eq!(code, 124, "{out}");
    assert_eq!(out["error"]["reason"], "expired");
    assert_eq!(request_of(&out)["state"], "expired");
    assert_eq!(consent_thens(&root, 2), ["asked", "expired"]);
    let (code, out, _) = run(&root, &home, &["consent", &id, "--json"]);
    assert_eq!(code, 124, "{out}");
}
