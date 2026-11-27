use std::collections::BTreeMap;

use notchling_engine::versions::{parse_probe, Mark, Method, Tool, Version, WorkerTools};

use super::versions_feed::{MacSide, MacTool, MacVersionsError};
use super::versions_model::{banner, rows};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn mac(entries: &[(Tool, &str, Method)]) -> Result<MacSide, MacVersionsError> {
    let mut tools = BTreeMap::new();
    for (tool, version, method) in entries {
        tools.insert(
            *tool,
            MacTool {
                installed: Some(v(version)),
                method: *method,
                reference: None,
                usable: false,
                installed_failed: false,
            },
        );
    }
    Ok(MacSide { tools })
}

fn worker(text: &str) -> WorkerTools {
    parse_probe(text, 1)
}

const NATIVE: &str = "claude.path=/home/w/.local/bin/claude\nclaude.link=/home/w/.local/share/claude/versions/2.1.323\nclaude.version=2.1.323 (Claude Code)\n";

#[test]
fn same_behind_ahead_marks() {
    let claude = |v: &str| mac(&[(Tool::Claude, v, Method::Native)]);
    let marks = |side| rows(&side, &worker(NATIVE), &[])[0].mark;
    assert_eq!(marks(claude("2.1.323")), Mark::Same);
    assert_eq!(marks(claude("2.1.324")), Mark::Behind);
    assert_eq!(marks(claude("2.1.300")), Mark::Ahead);
    let held = rows(&claude("2.1.324"), &worker(NATIVE), &[Tool::Claude]);
    assert_eq!(held[0].mark, Mark::Held);
    assert_eq!(held[0].mark_text, "Held");
}

#[test]
fn absent_tool_shows_not_installed() {
    let w = worker("claude.path=\ncodex.path=\n");
    let r = rows(&mac(&[(Tool::Claude, "2.1.324", Method::Native)]), &w, &[]);
    assert_eq!(r.len(), 4);
    assert!(r
        .iter()
        .all(|r| r.mark == Mark::NotInstalled && r.mark_text == "Not installed"));
    assert_eq!(r[0].text, "Not installed");
}

#[test]
fn no_daemon_shows_not_running_and_no_marks() {
    let err = Err(MacVersionsError::NotRunning);
    let r = rows(&err, &worker(NATIVE), &[]);
    assert_eq!(r[0].mark, Mark::Unknown);
    assert_eq!(r[0].mark_text, "Can't tell");
    assert_eq!(r[0].text, "2.1.323 · native installer");
    assert_eq!(banner(&err), Some("Notchling isn't running"));
    let absent = rows(&err, &worker("claude.path=\n"), &[]);
    assert_eq!(absent[0].mark, Mark::NotInstalled);
}

#[test]
fn brew_shows_cant_pin() {
    let brew = "codex.path=/opt/homebrew/bin/codex\ncodex.link=../Cellar/codex/0.166.0/bin/codex\ncodex.version=codex-cli 0.166.0\n";
    let r = rows(
        &mac(&[(Tool::Codex, "0.166.0", Method::Npm)]),
        &worker(brew),
        &[],
    );
    let codex = r.iter().find(|r| r.tool == Tool::Codex).unwrap();
    assert_eq!(codex.mark, Mark::CantPin);
    assert_eq!(codex.mark_text, "Homebrew can't install an exact version");
}

#[test]
fn unchecked_mac_version_is_labelled() {
    let r = rows(
        &mac(&[(Tool::Claude, "2.1.324", Method::Native)]),
        &worker(NATIVE),
        &[],
    );
    assert_eq!(
        r[0].text,
        "2.1.323 · native installer · the Mac has 2.1.324 (not checked yet)"
    );
    assert_eq!(banner(&mac(&[])), None);
}
