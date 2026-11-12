//! `--render chat` with the file panel open over a fixture Markdown file (`NOTCHLING_RENDER_FILE`):
//! needs a window server, so it is ignored by default. Run it with
//! `cargo test -p notchling-app -- --ignored` on a Mac. `NOTCHLING_FILE_PANEL_PNG=<path>` keeps
//! the picture there instead of deleting it.

#![cfg(target_os = "macos")]

use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn renders_the_file_panel_over_a_markdown_fixture() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("fixtures/chat");
    let file = manifest.join("fixtures/files/plan.md");
    let out = std::env::var_os("NOTCHLING_FILE_PANEL_PNG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("notchling-file-panel-{}.png", std::process::id()))
        });
    let keep = std::env::var_os("NOTCHLING_FILE_PANEL_PNG").is_some();
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat", "--out"])
        .arg(&out)
        .env("NOTCHLING_DATA_ROOT", &root)
        .env("NOTCHLING_RENDER_FILE", &file)
        .output()
        .expect("run notchling-app");
    assert!(
        output.status.success(),
        "exit {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let decoder = png::Decoder::new(File::open(&out).expect("the PNG exists"));
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG pixels");
    let bytes_per_pixel = info.line_size / info.width as usize;
    let distinct: HashSet<&[u8]> = pixels[..info.line_size * info.height as usize]
        .chunks_exact(bytes_per_pixel)
        .collect();
    assert!(
        distinct.len() > 64,
        "only {} distinct colors: the capture looks blank",
        distinct.len()
    );
    if !keep {
        std::fs::remove_file(&out).ok();
    }
}
