#![cfg(target_os = "macos")]

use std::collections::HashSet;
use std::fs::File;
use std::process::Command;

use notchling_app::views;

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn renders_kit_gallery_to_a_png_that_is_not_blank() {
    let out =
        std::env::temp_dir().join(format!("notchling-kit-gallery-{}.png", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "kit-gallery", "--out"])
        .arg(&out)
        .output()
        .expect("run notchling-app");
    assert!(
        output.status.success(),
        "exit {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("wrote "));

    let decoder = png::Decoder::new(File::open(&out).expect("the PNG exists"));
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG pixels");
    let spec = views::spec("kit-gallery").expect("the view exists");

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
