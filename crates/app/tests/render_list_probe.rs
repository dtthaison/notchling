//! The list probe (`--render chat-list-probe`, Task 2.1 of the Chat state split): GPUI's `list`
//! over 300 made-up rows, checked for what the conversation needs. Needs a window server and an
//! awake display, so it is ignored by default; run it in a release build under `caffeinate`:
//! `caffeinate -d -u cargo test --release -p notchling-app --test render_list_probe -- --ignored --nocapture`.
//! Writes nothing but the capture it is asked for, under the temp folder.

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
        // Keeps the view up (and tells the probe to run its checks); the probe exits by itself.
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

/// Gate G7 (Task 6.2): the kit's scroll bar over the list sits at the end of its track while the
/// list follows its end, and not once the list is scrolled up.
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
