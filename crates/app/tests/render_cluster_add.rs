#![cfg(target_os = "macos")]

use std::collections::HashSet;
use std::fs::File;
use std::path::PathBuf;
use std::process::Command;

use notchling_app::views;

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("NOTCHLING_RENDER_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&dir).expect("the output folder");
    dir
}

fn render(name: &str) {
    let spec = views::spec(name).unwrap_or_else(|| panic!("{name} is not a listed view"));
    let out = out_dir().join(format!("notchling-{name}.png"));
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", name, "--out"])
        .arg(&out)
        .output()
        .expect("run notchling-app");
    assert!(
        output.status.success(),
        "{name}: exit {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let decoder = png::Decoder::new(File::open(&out).expect("the PNG exists"));
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG pixels");
    assert!(
        info.width as f32 >= spec.size.0 && info.height as f32 >= spec.size.1,
        "{name}: {}x{} is smaller than {:?}",
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
        "{name}: only {} distinct colors",
        distinct.len()
    );
    println!("wrote {}", out.display());
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_cluster_add() {
    render("cluster-add");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_cluster_add_plan() {
    render("cluster-add-plan");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_cluster_add_running() {
    render("cluster-add-running");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_cluster_not_joined() {
    render("cluster-not-joined");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_machines() {
    render("machines-sheet");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_machines_add() {
    render("machines-sheet-add");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_machines_addresses() {
    render("machines-sheet-addresses");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_machines_tailscale() {
    render("machines-sheet-tailscale");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_machines_network() {
    render("machines-sheet-network");
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn render_cluster_update() {
    render("cluster-update");
}

#[test]
fn the_add_views_are_listed_and_named_for_the_map() {
    for name in [
        "cluster-add",
        "cluster-add-plan",
        "cluster-add-running",
        "cluster-add-stopped",
        "cluster-not-joined",
        "cluster-update",
        "machines-sheet",
        "machines-sheet-add",
        "machines-sheet-addresses",
        "machines-sheet-tailscale",
        "machines-sheet-network",
    ] {
        assert!(views::spec(name).is_some(), "{name}");
    }
    let sources = [
        include_str!("../src/cluster/add_view.rs"),
        include_str!("../src/cluster/add_plan_view.rs"),
        include_str!("../src/cluster/not_joined_view.rs"),
        include_str!("../src/cluster/updates_view.rs"),
        include_str!("../src/cluster/edit_addresses_view.rs"),
        include_str!("../src/cluster/from_tailscale_view.rs"),
    ]
    .join("\n");
    for part in [
        "Add Machine Sheet",
        "Machine Plan",
        "Not in the Cluster",
        "Update Machines",
        "Edit Addresses Sheet",
        "From Tailscale Sheet",
    ] {
        assert!(
            sources.contains(&format!("part(\"{part}\""))
                || sources.contains(&format!("part_at(\"{part}\"")),
            "{part}"
        );
    }
}
