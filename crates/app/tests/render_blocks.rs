//! The blocks check (`NOTCHLING_RENDER_CHECK_BLOCKS=1`) over the fixtures: with every group and
//! step open (`NOTCHLING_RENDER_EXPAND=1`), the texts the conversation made selectable are the
//! ones `chat::blocks::plain_blocks` gives from the model, in the same order. Needs a window
//! server, so it is ignored by default:
//! `cargo test --release --locked -p notchling-app --test render_blocks -- --ignored`.
//! Reads only the checked-in fixtures and a generated data root under the temp folder.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use notchling_app::chat::bench_fixture::files;
use notchling_engine::bridge::net::files::now_ms;

/// Renders Chat over `root` with everything expanded and returns the `blocks-check:` line.
fn check(root: &Path, expand: bool) -> String {
    let out = std::env::temp_dir().join(format!("notchling-blocks-{}.png", std::process::id()));
    let mut command = Command::new(env!("CARGO_BIN_EXE_notchling-app"));
    command
        .args(["--render", "chat", "--out"])
        .arg(&out)
        // Never the real ~/.claude: a fixture or generated root, named explicitly.
        .env("NOTCHLING_DATA_ROOT", root)
        .env("NOTCHLING_RENDER_CHECK_BLOCKS", "1");
    if expand {
        command.env("NOTCHLING_RENDER_EXPAND", "1");
    }
    let output = command.output().expect("run notchling-app");
    std::fs::remove_file(&out).ok();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "exit {:?}\nstdout: {stdout}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
        .lines()
        .find(|line| line.starts_with("blocks-check:"))
        .unwrap_or_else(|| panic!("no blocks-check line: {stdout}"))
        .to_owned()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

#[test]
#[ignore = "opens a window; needs a window server"]
fn the_blocks_match_what_the_chat_fixture_draws() {
    let line = check(&fixture("chat"), true);
    assert!(line.starts_with("blocks-check: ok "), "{line}");
}

#[test]
#[ignore = "opens a window; needs a window server"]
fn the_blocks_match_what_the_steps_fixture_draws() {
    let line = check(&fixture("chat-steps"), true);
    assert!(line.starts_with("blocks-check: ok "), "{line}");
    // Closed groups draw fewer texts than expanded ones, and the check holds for both.
    let closed = check(&fixture("chat-steps"), false);
    assert!(closed.starts_with("blocks-check: ok "), "{closed}");
}

#[test]
#[ignore = "opens a window; needs a window server"]
fn the_blocks_match_what_the_bench_fixture_draws() {
    let dir = std::env::temp_dir().join(format!("notchling-blocks-root-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files(30, now_ms()) {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
        std::fs::write(&path, text).expect("write the file");
    }
    let line = check(&dir, true);
    std::fs::remove_dir_all(&dir).ok();
    assert!(line.starts_with("blocks-check: ok "), "{line}");
}
