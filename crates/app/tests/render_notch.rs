#![cfg(target_os = "macos")]

use std::collections::HashSet;
use std::fs::File;
use std::process::Command;

use notchling_app::notch::renders::VIEWS;

fn is_black(pixel: &[u8]) -> bool {
    pixel[..3] == [0, 0, 0]
}

#[test]
#[ignore = "opens a window per state and captures it; needs a window server"]
fn renders_every_notch_state_over_the_fixture() {
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
        let rows: Vec<&[u8]> = pixels[..info.line_size * info.height as usize]
            .chunks_exact(info.line_size)
            .collect();
        let middle = (info.width as usize / 2) * bpp;
        assert!(
            is_black(&rows[0][middle..middle + bpp]),
            "{}: no black shape at the top centre",
            spec.name
        );
        let distinct: HashSet<&[u8]> = rows.iter().flat_map(|r| r.chunks_exact(bpp)).collect();
        let least = if matches!(spec.name, "notch-collapsed" | "notch-pet" | "notch-perch") {
            3
        } else {
            64
        };
        assert!(
            distinct.len() >= least,
            "{}: only {} distinct colors",
            spec.name,
            distinct.len()
        );
        std::fs::remove_file(&out).ok();
    }
}
