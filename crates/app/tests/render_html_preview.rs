#![cfg(target_os = "macos")]

use std::fs::File;
use std::path::Path;
use std::process::Command;

fn near(pixel: &[u8], rgb: [u8; 3]) -> bool {
    (0..3).all(|i| (pixel[i] as i16 - rgb[i] as i16).abs() <= 24)
}

fn count(path: &Path, targets: &[[u8; 3]]) -> Vec<usize> {
    let decoder = png::Decoder::new(File::open(path).expect("the PNG exists"));
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG pixels");
    let bytes_per_pixel = info.line_size / info.width as usize;
    let rows = &pixels[..info.line_size * info.height as usize];
    targets
        .iter()
        .map(|target| {
            rows.chunks_exact(bytes_per_pixel)
                .filter(|p| near(p, *target))
                .count()
        })
        .collect()
}

#[test]
#[ignore = "opens a window with a web view and captures it; needs a window server and an unlocked display"]
fn renders_an_html_file_as_a_page_with_scripts_off() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("fixtures/chat");
    let file = manifest.join("fixtures/files/site/index.html");
    let out = std::env::var_os("NOTCHLING_HTML_PREVIEW_PNG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("notchling-html-preview-{}.png", std::process::id()))
        });
    let keep = std::env::var_os("NOTCHLING_HTML_PREVIEW_PNG").is_some();
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat", "--out"])
        .arg(&out)
        .env("NOTCHLING_DATA_ROOT", &root)
        .env("NOTCHLING_RENDER_FILE", &file)
        .env("NOTCHLING_RENDER_HTML", "web")
        .env("NOTCHLING_RENDER_APPEARANCE", "light")
        .env("NOTCHLING_RENDER_BENCH", "3")
        .env("NOTCHLING_RENDER_BENCH_MODE", "idle")
        .output()
        .expect("run notchling-app");
    assert!(
        output.status.success(),
        "exit {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let found = count(
        &out,
        &[
            [0xff, 0xf7, 0xe6],
            [0xb4, 0x38, 0x0f],
            [0xff, 0xe0, 0xa8],
            [0x2e, 0x7d, 0x32],
        ],
    );
    assert!(
        found[0] > 5_000,
        "the page's own background is missing ({}): the web view did not paint the file or its style",
        found[0]
    );
    assert!(
        found[1] > 100,
        "the page's heading and picture are missing ({}): the style or the logo beside the file did not load",
        found[1]
    );
    assert!(
        found[2] > 500,
        "the status chip is missing ({}): the style did not reach the page",
        found[2]
    );
    assert_eq!(
        found[3], 0,
        "the page's script turned its chip green: scripts ran in a page that must have them off"
    );
    if !keep {
        std::fs::remove_file(&out).ok();
    }
}
