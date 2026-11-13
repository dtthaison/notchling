#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

fn copy_check(how: &str) -> String {
    run_copy_check(how, false)
}

fn copy_check_in_file_panel(how: &str) -> String {
    run_copy_check(how, true)
}

fn run_copy_check(how: &str, in_file_panel: bool) -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("fixtures/chat");
    let out = std::env::temp_dir().join(format!(
        "notchling-copy-{}-{how}-{in_file_panel}.png",
        std::process::id()
    ));
    let mut command = Command::new(env!("CARGO_BIN_EXE_notchling-app"));
    command
        .args(["--render", "chat", "--out"])
        .arg(&out)
        .env("NOTCHLING_DATA_ROOT", &root)
        .env("NOTCHLING_RENDER_CHECK_COPY", how)
        .env("NOTCHLING_RENDER_FORCE_FRAMES", "1");
    if in_file_panel {
        command
            .env(
                "NOTCHLING_RENDER_FILE",
                manifest.join("fixtures/files/plan.md"),
            )
            .env("NOTCHLING_RENDER_CHECK_COPY_IN", "file");
    }
    let output = command.output().expect("run notchling-app");
    std::fs::remove_file(&out).ok();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success() && stdout.contains("copy-check: ok"),
        "exit {:?}\nstdout: {stdout}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

#[test]
#[ignore = "opens a window, sends it events and writes the clipboard; needs a window server"]
fn command_c_copies_a_selection_made_with_the_pointer() {
    copy_check("key");
}

#[test]
#[ignore = "opens a window, sends it events and writes the clipboard; needs a window server"]
fn the_copy_action_copies_a_selection_made_with_the_pointer() {
    copy_check("action");
}

#[test]
#[ignore = "opens a window, sends it events and writes the clipboard; needs a window server"]
fn the_right_click_menu_copies_a_selection_made_with_the_pointer() {
    copy_check("menu");
}

#[test]
#[ignore = "opens a window, sends it events and writes the clipboard; needs a window server"]
fn command_c_copies_a_selection_in_the_file_panel() {
    copy_check_in_file_panel("key");
}

#[test]
#[ignore = "opens a window, sends it events and writes the clipboard; needs a window server"]
fn the_right_click_menu_copies_a_selection_in_the_file_panel() {
    copy_check_in_file_panel("menu");
}
