#![cfg(unix)]

mod common;

use common::audit::{audit_dir, key_path, kinds, lines, month_files, of_kind, verdict};
use common::{connect, fixture_root, home, scratch, Daemon};
use notchling_events::audit_check::CheckResult;
use notchling_local_api::{ops, ClientKind};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

fn refuse_one(root: &Path) {
    let mut agent = connect(root, ClientKind::Cli);
    assert!(agent
        .call(ops::SESSION_REPLY, json!({"id": "x", "text": "hi"}))
        .is_err());
}

fn verify(root: &Path) -> Value {
    let mut agent = connect(root, ClientKind::Cli);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let report = agent.call(ops::LOG_VERIFY, json!({"full": true})).unwrap();
        if report["sealing"] != "starting" || Instant::now() > deadline {
            return report;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for(root: &Path, kind: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while of_kind(root, kind).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn daemon_run_writes_sealed_month_and_checks_intact() {
    let dir = scratch("log-audit-run");
    let root = fixture_root(&dir.0);
    let home = home(&dir.0);
    let mut daemon = Daemon::start(&root, &home);
    refuse_one(&root);
    wait_for(&root, "api.refused");
    let report = verify(&root);
    assert_eq!(report["sealing"], "sealed", "{report}");
    assert_eq!(report["result"], "intact", "{report}");
    daemon.stop();

    let seen = kinds(&root);
    for kind in ["daemon.started", "api.refused", "daemon.stopped"] {
        assert!(seen.iter().any(|k| k == kind), "{kind} in {seen:?}");
    }
    assert!(lines(&root).iter().all(|line| line["seal"].is_string()));
    assert_eq!(month_files(&root).len(), 1);
    assert!(key_path(&root).exists());
    assert_eq!(verdict(&root).result, CheckResult::Intact);
    let refused = &of_kind(&root, "api.refused")[0];
    assert_eq!(refused["f"]["op"], ops::SESSION_REPLY, "{refused}");
}

#[test]
fn restart_continues_the_chain() {
    let dir = scratch("log-audit-restart");
    let root = fixture_root(&dir.0);
    let home = home(&dir.0);
    for _ in 0..2 {
        let mut daemon = Daemon::start(&root, &home);
        refuse_one(&root);
        daemon.stop();
    }
    assert_eq!(of_kind(&root, "daemon.started").len(), 2);
    let numbers: Vec<u64> = lines(&root)
        .iter()
        .map(|line| line["n"].as_u64().unwrap())
        .collect();
    let first = numbers[0];
    let expected: Vec<u64> = (first..first + numbers.len() as u64).collect();
    assert_eq!(numbers, expected, "one chain, no gaps");
    assert!(matches!(
        verdict(&root).result,
        CheckResult::Intact | CheckResult::Resumed
    ));
}

#[test]
fn edited_month_is_reported_by_log_verify() {
    let dir = scratch("log-audit-edit");
    let root = fixture_root(&dir.0);
    let home = home(&dir.0);
    let mut first = Daemon::start(&root, &home);
    refuse_one(&root);
    first.stop();

    let (_, path) = month_files(&root).remove(0);
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = text.replacen("\"kind\":\"api.refused\"", "\"kind\":\"api.refusef\"", 1);
    assert_ne!(edited, text);
    std::fs::write(&path, edited).unwrap();

    let mut second = Daemon::start(&root, &home);
    let report = verify(&root);
    assert_eq!(report["result"], "broken", "{report}");
    assert!(report["first_bad_n"].is_u64(), "{report}");
    second.stop();
}

#[test]
fn missing_key_file_runs_unsealed() {
    let dir = scratch("log-audit-nokey");
    let root = fixture_root(&dir.0);
    let home = home(&dir.0);
    let mut first = Daemon::start(&root, &home);
    refuse_one(&root);
    first.stop();
    let before = lines(&root).len();

    std::fs::remove_file(key_path(&root)).unwrap();
    let mut second = Daemon::start(&root, &home);
    refuse_one(&root);
    let deadline = Instant::now() + Duration::from_secs(10);
    while of_kind(&root, "daemon.started").len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let report = verify(&root);
    assert_eq!(report["sealing"], "unsealed", "{report}");
    assert_eq!(report["result"], "unsealed", "{report}");
    second.stop();

    let later: Vec<Value> = lines(&root).into_iter().skip(before).collect();
    assert!(!later.is_empty());
    assert!(
        later.iter().all(|line| line["unsealed"] == "key_missing"),
        "{later:?}"
    );
    assert!(
        !key_path(&root).exists(),
        "no key is made behind the user's back"
    );
}

#[test]
fn legacy_audit_is_moved_and_imported() {
    let dir = scratch("log-audit-legacy");
    let root = fixture_root(&dir.0);
    let home = home(&dir.0);
    let legacy = [
        r#"{"time_ms":1000,"op":"session.reply","tier":"app","peer":"/Applications/Notchling.app/Contents/MacOS/notchling-app","result":"ok"}"#,
        r#"{"time_ms":2000,"op":"vault.pending","tier":"agent","peer":"/usr/bin/curl","result":"forbidden"}"#,
        r#"{"time_ms":6000,"op":"accounts.switch","tier":"app","peer":"/Applications/Notchling.app/Contents/MacOS/notchling-app","result":"ok"}"#,
    ]
    .join("\n")
        + "\n";
    let old = root.join("notchling/audit.jsonl");
    std::fs::write(&old, legacy).unwrap();

    let mut daemon = Daemon::start_for_app(&root, &home);
    assert!(!old.exists(), "the old file moved");
    assert!(audit_dir(&root).join("legacy/audit.jsonl").exists());
    let mut app = connect(&root, ClientKind::App);
    let reply = app.call(ops::ACTIVITY_LIST, json!({"limit": 500})).unwrap();
    let imported = reply["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["source"] == "legacy")
        .count();
    assert_eq!(imported, 3, "{reply}");
    daemon.stop();
}
