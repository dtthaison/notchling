use std::process::Command;

fn machines() -> Command {
    Command::new(env!("CARGO_BIN_EXE_notchling-machines"))
}

#[test]
fn version_help_and_a_bad_argument() {
    let out = machines()
        .arg("--version")
        .output()
        .expect("run notchling-machines");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("notchling-machines "));

    let out = machines()
        .arg("--help")
        .output()
        .expect("run notchling-machines");
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("--live") && help.contains("--vault") && help.contains("machines-fix"));

    let out = machines()
        .arg("--frobnicate")
        .output()
        .expect("run notchling-machines");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument"));

    let out = machines()
        .args(["--render", "machines", "--out", "/tmp/x.png", "--live"])
        .output()
        .expect("run notchling-machines");
    assert_eq!(out.status.code(), Some(2));
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "opens a window per view and captures it; needs a window server"]
fn renders_every_machines_view_over_the_fixture() {
    use notchling_app::machines::renders::VIEWS;
    use std::collections::HashSet;
    use std::fs::File;

    for spec in VIEWS {
        let out = std::env::temp_dir().join(format!(
            "notchling-{}-{}.png",
            spec.name,
            std::process::id()
        ));
        let output = machines()
            .args(["--render", spec.name, "--out"])
            .arg(&out)
            .output()
            .expect("run notchling-machines");
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
