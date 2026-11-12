//! Copying text selected in Chat's conversation, end to end: `--render chat` over the checked-in
//! fixture selects text with the pointer and then copies it (`NOTCHLING_RENDER_CHECK_COPY`), and the
//! clipboard must hold what the selection says. Needs a window server, so it is ignored by default;
//! run it with `cargo test -p notchling-app --test render_copy -- --ignored --test-threads=1` on a
//! Mac. It overwrites the Mac's clipboard.
//!
//! In the fixture nothing in the window has the keyboard (the reply box is off, as it is for a
//! session the app can't type into, and after a click elsewhere), which is the case that broke:
//! GPUI sends a key to the focused element's ancestors, and with no focus that is the window's
//! outermost node, so a handler on Chat's root never saw ⌘C.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

/// Runs the copy check in `how` mode over the conversation; the render's output.
fn copy_check(how: &str) -> String {
    run_copy_check(how, false)
}

/// The same with the file panel open over a fixture file, and the selection made in the panel.
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
        // Never the real ~/.claude: the fixture, named explicitly.
        .env("NOTCHLING_DATA_ROOT", &root)
        .env("NOTCHLING_RENDER_CHECK_COPY", how)
        // A locked screen sends no frames: draw them by hand.
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
