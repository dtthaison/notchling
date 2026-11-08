//! `--render banner|new-session|trust-folder|settings` end to end over the checked-in fixture:
//! needs a window server, so it is ignored by default. Run it with
//! `cargo test -p notchling-app -- --ignored` on a Mac with a display.

#![cfg(target_os = "macos")]

use std::collections::HashSet;
use std::fs::File;
use std::process::Command;

use notchling_app::sheets::renders::VIEWS;

#[test]
#[ignore = "opens a window per sheet and captures it; needs a window server"]
fn renders_every_sheet_over_the_fixture() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/notch");
    for spec in VIEWS {
        let out = std::env::temp_dir().join(format!(
            "notchling-{}-{}.png",
            spec.name,
            std::process::id()
        ));
        let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
            .args(["--render", spec.name, "--out"])
            .arg(&out)
            // Never the real ~/.claude: the fixture, named explicitly.
            .env("NOTCHLING_DATA_ROOT", fixture)
            .output()
            .expect("run notchling-app");
        assert!(
            output.status.success(),
            "{}: exit {:?}\nstderr: {}",
            spec.name,
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let decoder = png::Decoder::new(File::open(&out).expect("the PNG exists"));
        let mut reader = decoder.read_info().expect("a PNG header");
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).expect("PNG pixels");
        assert!(
            info.width as f32 >= spec.size.0 && info.height as f32 >= spec.size.1,
            "{}: {}x{} is smaller than {:?}",
            spec.name,
            info.width,
            info.height,
            spec.size
        );
        let bpp = info.line_size / info.width as usize;
        let distinct: HashSet<&[u8]> = pixels[..info.line_size * info.height as usize]
            .chunks_exact(bpp)
            .collect();
        assert!(
            distinct.len() >= 64,
            "{}: only {} distinct colors",
            spec.name,
            distinct.len()
        );
        std::fs::remove_file(&out).ok();
    }
}
