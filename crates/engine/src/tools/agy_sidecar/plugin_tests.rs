use super::plugin::{
    install, installed_for, remove, sidecar_dir, sidecar_json, state, Plugin, MARK,
};
use super::Roots;
use serde_json::json;
use std::path::PathBuf;

fn roots(name: &str) -> Roots {
    let home = std::env::temp_dir().join(format!("nl-plugin-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let roots = Roots::under(&home);
    std::fs::create_dir_all(roots.config.join("plugins")).unwrap();
    roots
}

fn bin() -> PathBuf {
    PathBuf::from("/Applications/Notchling.app/Contents/MacOS/notchling")
}

#[test]
fn the_sidecar_runs_the_installed_binary_with_one_subcommand_and_no_shell() {
    let value = sidecar_json(&bin()).unwrap();
    assert_eq!(
        value["command"],
        "/Applications/Notchling.app/Contents/MacOS/notchling"
    );
    assert_eq!(value["args"], json!(["agy-sidecar"]));
    assert_eq!(value["restart_policy"], "always");
    assert!(sidecar_json(&PathBuf::from("notchling")).is_err());
}

#[test]
fn install_writes_the_plugin_with_its_mark_and_is_idempotent() {
    let r = roots("install");
    assert_eq!(state(&r), Plugin::Absent);
    assert!(install(&r, &bin()).unwrap());
    assert_eq!(state(&r), Plugin::Ours);
    assert!(r.plugin_dir().join(MARK).is_file());
    assert!(r.plugin_dir().join("plugin.json").is_file());
    assert!(installed_for(&r, &bin()));
    assert!(!install(&r, &bin()).unwrap());
}

#[test]
fn a_moved_binary_rewrites_only_the_sidecar_file() {
    let r = roots("moved");
    install(&r, &bin()).unwrap();
    let other = PathBuf::from("/opt/n/notchling");
    assert!(!installed_for(&r, &other));
    assert!(install(&r, &other).unwrap());
    assert!(installed_for(&r, &other));
}

#[test]
fn a_plugin_folder_that_is_not_ours_is_never_touched() {
    let r = roots("foreign");
    std::fs::create_dir_all(r.plugin_dir()).unwrap();
    std::fs::write(
        r.plugin_dir().join("plugin.json"),
        "{\"name\":\"notchling\"}",
    )
    .unwrap();
    assert_eq!(state(&r), Plugin::Foreign);
    assert!(install(&r, &bin()).is_err());
    assert_eq!(remove(&r), Ok(false));
    assert!(r.plugin_dir().join("plugin.json").is_file());
    assert!(!sidecar_dir(&r).exists());
}

#[test]
fn remove_takes_only_what_it_wrote_and_the_folder_when_it_is_then_empty() {
    let r = roots("remove");
    install(&r, &bin()).unwrap();
    assert_eq!(remove(&r), Ok(true));
    assert!(!r.plugin_dir().exists());
    assert_eq!(remove(&r), Ok(false));
    install(&r, &bin()).unwrap();
    std::fs::create_dir_all(r.plugin_dir().join("rules")).unwrap();
    std::fs::write(r.plugin_dir().join("rules/AGENTS.md"), "x").unwrap();
    assert_eq!(remove(&r), Ok(true));
    assert!(r.plugin_dir().join("rules/AGENTS.md").is_file());
    assert!(r.plugin_dir().join(MARK).is_file());
    assert!(!sidecar_dir(&r).exists());
}

#[cfg(unix)]
#[test]
fn a_link_in_the_way_is_refused() {
    let r = roots("link");
    let target = r.config.join("elsewhere");
    std::fs::create_dir_all(&target).unwrap();
    std::os::unix::fs::symlink(&target, r.plugin_dir()).unwrap();
    assert_eq!(state(&r), Plugin::Foreign);
    assert!(install(&r, &bin()).is_err());
    assert!(std::fs::read_dir(&target).unwrap().next().is_none());
}

#[test]
fn nothing_is_written_while_antigravity_has_no_config_folder() {
    let home = std::env::temp_dir().join(format!("nl-plugin-nocfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let r = Roots::under(&home);
    assert!(install(&r, &bin()).is_err());
    assert!(!home.exists());
}
