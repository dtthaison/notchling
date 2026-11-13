#![cfg(target_os = "macos")]

use std::process::Command;

use serde_json::Value;

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn the_list_does_what_the_conversation_needs() {
    let out = std::env::temp_dir().join(format!("notchling-list-probe-{}.png", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat-list-probe", "--out"])
        .arg(&out)
        .env("NOTCHLING_RENDER_BENCH", "120")
        .output()
        .expect("run notchling-app");
    std::fs::remove_file(&out).ok();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("list-probe: "))
        .unwrap_or_else(|| panic!("no list-probe line\nstdout: {stdout}\nstderr: {stderr}"));
    println!("list-probe: {line}");
    let gates: Value = serde_json::from_str(line).expect("list-probe is JSON");
    for gate in ["g1", "g2", "g3", "g4", "g5"] {
        assert_eq!(gates[gate]["pass"], true, "{gate} failed: {}", gates[gate]);
    }
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn the_bar_sits_at_the_end_of_its_track_while_the_list_follows() {
    let out = std::env::temp_dir().join(format!("notchling-list-bar-{}.png", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat-list-probe", "--out"])
        .arg(&out)
        .env("NOTCHLING_RENDER_BENCH", "120")
        .output()
        .expect("run notchling-app");
    std::fs::remove_file(&out).ok();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("list-probe: "))
        .unwrap_or_else(|| panic!("no list-probe line\nstdout: {stdout}\nstderr: {stderr}"));
    let gates: Value = serde_json::from_str(line).expect("list-probe is JSON");
    assert_eq!(gates["g7"]["pass"], true, "g7 failed: {}", gates["g7"]);
}
