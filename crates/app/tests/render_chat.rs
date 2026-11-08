//! `--render chat --data-root tests/fixtures/chat-root` end to end: needs a window server, so it
//! is ignored by default. Run it with `cargo test -p notchling-app -- --ignored` on a Mac.

#![cfg(target_os = "macos")]

use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::process::Command;

use notchling_app::views;

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn renders_chat_over_the_fixture_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/chat-root");
    let out = std::env::temp_dir().join(format!("notchling-chat-{}.png", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat", "--out"])
        .arg(&out)
        .arg("--data-root")
        .arg(&root)
        .output()
        .expect("run notchling-app");
    assert!(
        output.status.success(),
        "exit {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let decoder = png::Decoder::new(File::open(&out).expect("the PNG exists"));
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG pixels");
    let spec = views::spec("chat").expect("the view exists");
    assert!(
        info.width as f32 >= spec.size.0 && info.height as f32 >= spec.size.1,
        "{}x{} is smaller than the {}x{} window",
        info.width,
        info.height,
        spec.size.0,
        spec.size.1
    );
    let bytes_per_pixel = info.line_size / info.width as usize;
    let distinct: HashSet<&[u8]> = pixels[..info.line_size * info.height as usize]
        .chunks_exact(bytes_per_pixel)
        .collect();
    assert!(
        distinct.len() > 64,
        "only {} distinct colors: the capture looks blank",
        distinct.len()
    );
    std::fs::remove_file(&out).ok();
}
