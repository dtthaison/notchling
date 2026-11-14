#![cfg(unix)]

mod common;

use common::{connect, fixture_root, home, notchling, scratch, Daemon};
use notchling_local_api::{ops, ClientKind};
use serde_json::json;
use std::path::{Path, PathBuf};

const CANARY: &str = "CANARY-7f3a";

fn files_under(path: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let p = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            files_under(&p, out);
        } else if kind.is_file() {
            out.push(p);
        }
    }
}

fn holds(bytes: &[u8]) -> bool {
    bytes
        .windows(CANARY.len())
        .any(|window| window == CANARY.as_bytes())
}

#[test]
fn no_planted_text_reaches_any_file_under_the_data_root() {
    let dir = scratch("log-canary");
    let root = fixture_root(&dir.0);
    let home = home(&dir.0);
    let mut daemon = Daemon::start_for_app(&root, &home);

    let mut app = connect(&root, ClientKind::App);
    let _ = app.call(
        ops::SETTINGS_SET_SWITCH,
        json!({"switch": "vault", "on": true}),
    );
    let _ = app.call(
        ops::VAULT_SAVE,
        json!({"entry": {"name": "gh", "kind": "token"}, "value": {"secret": CANARY}}),
    );
    let _ = app.call(
        ops::VAULT_SAVE,
        json!({"entry": {"name": CANARY, "kind": "token"}, "value": {"secret": CANARY}}),
    );

    let command = format!("echo {CANARY}");
    let _ = notchling(&root, &home, &["secret", "gh", "--", &command]);
    let _ = notchling(&root, &home, &["secret", CANARY, "--", &command]);
    let _ = notchling(&root, &home, &["settings", "set", "limitMode", CANARY]);
    let _ = notchling(&root, &home, &["settings", "set", CANARY, CANARY]);

    let mut agent = connect(&root, ClientKind::Cli);
    let _ = agent.call(ops::SESSION_REPLY, json!({"id": "x", "text": CANARY}));
    let _ = agent.call(ops::SESSION_REPLY, json!({"id": CANARY, "text": "hi"}));
    let missing = format!("/tmp/{CANARY}/missing");
    let _ = notchling(&root, &home, &["page", &missing]);
    let _ = agent.call(ops::PAGES_FIND, json!({"query": missing}));

    daemon.stop();

    let seen = common::audit::kinds(&root);
    assert!(seen.iter().any(|k| k == "session.reply"), "{seen:?}");
    assert!(seen.iter().any(|k| k == "settings.agent"), "{seen:?}");

    let mut files = Vec::new();
    files_under(&dir.0, &mut files);
    let mut checked = 0;
    for file in files {
        if file.file_name().is_some_and(|n| n == "crash.log") {
            continue;
        }
        let bytes = std::fs::read(&file).unwrap_or_default();
        checked += 1;
        if holds(&bytes) && std::env::var_os("CANARY_DUMP").is_some() {
            for line in String::from_utf8_lossy(&bytes)
                .lines()
                .filter(|l| l.contains(CANARY))
            {
                eprintln!("{}: {line}", file.display());
            }
        }
        assert!(!holds(&bytes), "{} holds the planted text", file.display());
    }
    assert!(checked > 10, "the scan saw the data root: {checked} files");
    assert!(
        root.join("notchling/daemon/events.db").exists(),
        "events.db was scanned"
    );
}
