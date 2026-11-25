use std::path::Path;

use notchling_engine::versions::{Method, Tool, Version};
use serde_json::json;

use super::data::Setup;
use super::versions_feed::{
    mac_side, DaemonVersions, FixedVersions, MacSide, MacVersions, MacVersionsError,
};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

#[test]
fn reply_maps_to_mac_side() {
    let reply = json!({"tools": [
        {"tool": "claude", "installed": {"version": "2.1.324", "sha256": "ab"}, "method": "native", "path": "/h/.local/bin/claude", "problems": [], "running": [], "reference": null, "usable": false, "failed": [], "check": null},
        {"tool": "antigravity", "installed": {"version": null, "sha256": null}, "method": "standalone", "reference": null, "usable": false},
        {"tool": "codex", "installed": null, "method": null, "reference": {"version": "0.166.0"}, "usable": true},
        {"tool": "gemini", "installed": {"version": "9.9.9"}},
    ]});
    let side = mac_side(&reply).unwrap();
    assert_eq!(side.tools.len(), 3);
    let claude = &side.tools[&Tool::Claude];
    assert_eq!(claude.installed, Some(v("2.1.324")));
    assert_eq!(claude.method, Method::Native);
    assert!(!claude.usable);
    assert_eq!(side.tools[&Tool::Agy].installed, None);
    assert_eq!(side.tools[&Tool::Codex].method, Method::Unknown);
    assert_eq!(side.tools[&Tool::Codex].reference, Some(v("0.166.0")));
    let compare = side.compare_with();
    assert_eq!(compare[&Tool::Claude], v("2.1.324"));
    assert_eq!(compare[&Tool::Codex], v("0.166.0"));
    assert!(!compare.contains_key(&Tool::Agy));
}

#[test]
fn a_reply_without_tools_is_bad() {
    assert!(matches!(
        mac_side(&json!({"nope": 1})),
        Err(MacVersionsError::Bad(_))
    ));
    let hostile = json!({"tools": [{"tool": "claude", "installed": {"version": "2.1.3; rm"}}]});
    assert_eq!(
        mac_side(&hostile).unwrap().tools[&Tool::Claude].installed,
        None
    );
}

#[test]
fn no_socket_is_not_running() {
    let dir = std::env::temp_dir().join(format!("nl-vfeed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let feed = DaemonVersions {
        socket: dir.join("notchlingd.sock"),
    };
    assert_eq!(feed.get(), Err(MacVersionsError::NotRunning));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fixed_versions_answer_as_given() {
    let side = MacSide::default();
    assert_eq!(FixedVersions(Ok(side.clone())).get(), Ok(side));
    assert_eq!(
        FixedVersions(Err(MacVersionsError::NotRunning)).get(),
        Err(MacVersionsError::NotRunning)
    );
}

#[test]
fn fixture_setup_uses_fixed_versions() {
    let render = Setup::for_render(None);
    let side = render.versions().get().unwrap();
    assert_eq!(side.compare_with()[&Tool::Claude], v("2.1.324"));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/machines");
    let run = Setup::for_run(Some(&fixtures), true, false).unwrap();
    assert!(run.fixture);
    assert_eq!(run.versions().get().unwrap(), side);
}
