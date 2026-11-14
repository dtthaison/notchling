use notchling_engine::versions::{parse_probe, Mark, Tool, WorkerTools};
use serde_json::json;

use super::versions_feed::mac_side;
use super::versions_model::{banner, rows};

const NATIVE: &str = "claude.path=/home/w/.local/bin/claude\nclaude.link=/home/w/.local/share/claude/versions/2.1.323\nclaude.version=2.1.323 (Claude Code)\n";

fn worker() -> WorkerTools {
    parse_probe(NATIVE, 1)
}

fn reply(
    installed: &str,
    reference: serde_json::Value,
    usable: bool,
    failed: serde_json::Value,
) -> serde_json::Value {
    json!({"tools": [{
        "tool": "claude",
        "installed": {"version": installed, "sha256": "ab"},
        "method": "native",
        "reference": reference,
        "usable": usable,
        "failed": failed,
    }]})
}

fn reference(version: &str) -> serde_json::Value {
    json!({"version": version, "sha256": "ab", "result": "partly", "partly": true, "at": 1, "unchecked": []})
}

#[test]
fn marks_follow_the_usable_reference() {
    let side = mac_side(&reply("2.1.325", reference("2.1.323"), true, json!([])));
    let r = rows(&side, &worker(), &[]);
    assert_eq!(r[0].mark, Mark::Same);
    assert_eq!(
        r[0].text,
        "2.1.323 · native installer · the Mac has 2.1.323"
    );
    assert_eq!(banner(&side), None);
}

#[test]
fn stale_reference_shows_out_of_date_and_no_marks() {
    let side = mac_side(&reply("2.1.325", reference("2.1.323"), false, json!([])));
    let r = rows(&side, &worker(), &[]);
    assert_eq!(r[0].mark, Mark::Unknown);
    assert_eq!(r[0].mark_text, "Can't tell");
    assert_eq!(banner(&side), Some("The Mac's check is out of date"));
}

#[test]
fn failed_version_is_never_the_mark_target() {
    let failed = json!([{"version": "2.1.325", "part": "transcripts", "at": 1, "why": "x"}]);
    let side = mac_side(&reply("2.1.325", json!(null), false, failed));
    assert!(!side
        .as_ref()
        .unwrap()
        .compare_with()
        .contains_key(&Tool::Claude));
    let r = rows(&side, &worker(), &[]);
    assert_eq!(r[0].mark, Mark::Unknown);
    let other = json!([{"version": "2.1.326", "part": "transcripts", "at": 1, "why": "x"}]);
    let side = mac_side(&reply("2.1.325", json!(null), false, other)).unwrap();
    assert!(side.compare_with().contains_key(&Tool::Claude));
}
