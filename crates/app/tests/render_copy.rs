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

/// Runs the copy check in `how` mode; the render's output.
fn copy_check(how: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/chat");
    let out = std::env::temp_dir().join(format!("notchling-copy-{}-{how}.png", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat", "--out"])
        .arg(&out)
        // Never the real ~/.claude: the fixture, named explicitly.
        .env("NOTCHLING_DATA_ROOT", &root)
        .env("NOTCHLING_RENDER_CHECK_COPY", how)
        // A locked screen sends no frames: draw them by hand.
        .env("NOTCHLING_RENDER_FORCE_FRAMES", "1")
        .output()
        .expect("run notchling-app");
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
