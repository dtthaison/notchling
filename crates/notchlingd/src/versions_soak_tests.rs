use crate::versions_check_tests::{
    check, claude_transcripts, get, native_claude, out, ready, rig, Rig,
};
use serde_json::Value;

fn checked(rig: &Rig) {
    native_claude(rig, "2.1.324", "claude 2.1.324");
    claude_transcripts(rig);
    ready(rig);
    rig.launcher.push_output(out("2.1.324 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    check(rig, "claude");
    rig.ops.wait_checks();
}

#[cfg(unix)]
#[test]
fn the_pass_counts_real_turns_under_the_reference_version() {
    let rig = rig("soak-turns");
    checked(&rig);
    assert_eq!(get(&rig, "claude")["soak"], Value::Null);
    rig.ops.soak_pass();
    let soak = get(&rig, "claude")["soak"].clone();
    assert_eq!(soak["since"], "2.1.324");
    assert_eq!(soak["turns"], 1);
    assert_eq!(soak["unknown"], 0);
    assert_eq!(soak["line"], "Since 2.1.324: 1 turn read, nothing unknown");
}

#[cfg(unix)]
#[test]
fn unknown_lines_show_and_gate_nothing() {
    let rig = rig("soak-unknown");
    checked(&rig);
    rig.tmp.write(
        "home/.claude/projects/proj/s2.jsonl",
        "{\"type\":\"hologram\",\"version\":\"2.1.324\"}\n{\"type\":\"hologram\",\"version\":\"2.1.324\"}\n",
    );
    let before = get(&rig, "claude")["reference"].clone();
    rig.ops.soak_pass();
    let row = get(&rig, "claude");
    assert_eq!(row["soak"]["unknown"], 2);
    assert_eq!(row["reference"], before);
    assert_eq!(row["usable"], true);
    assert_eq!(row["failed"], serde_json::json!([]));
}

#[cfg(unix)]
#[test]
fn a_second_pass_does_not_count_the_same_lines_again() {
    let rig = rig("soak-twice");
    checked(&rig);
    rig.ops.soak_pass();
    rig.ops.soak_pass();
    assert_eq!(get(&rig, "claude")["soak"]["turns"], 1);
}

#[cfg(unix)]
#[test]
fn a_tool_without_a_reference_has_no_soak() {
    let rig = rig("soak-none");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    claude_transcripts(&rig);
    ready(&rig);
    rig.ops.soak_pass();
    assert_eq!(get(&rig, "claude")["soak"], Value::Null);
}
