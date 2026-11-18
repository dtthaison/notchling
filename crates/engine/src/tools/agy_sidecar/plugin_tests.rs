use super::plugin::{
    install, installed_for, preflight, remove, sidecar_dir, sidecar_json, state, Plugin, MARK,
    MARK_TEXT,
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
    assert!(install(&r, &bin(), 1).unwrap());
    assert_eq!(state(&r), Plugin::Ours);
    assert!(r.plugin_dir().join(MARK).is_file());
    assert!(r.plugin_dir().join("plugin.json").is_file());
    assert!(installed_for(&r, &bin()));
    assert!(!install(&r, &bin(), 1).unwrap());
}

#[test]
fn a_moved_binary_rewrites_only_the_sidecar_file() {
    let r = roots("moved");
    install(&r, &bin(), 1).unwrap();
    let other = PathBuf::from("/opt/n/notchling");
    assert!(!installed_for(&r, &other));
    assert!(install(&r, &other, 2).unwrap());
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
    assert!(install(&r, &bin(), 1).is_err());
    assert_eq!(remove(&r), Ok(false));
    assert!(r.plugin_dir().join("plugin.json").is_file());
    assert!(!sidecar_dir(&r).exists());
}

#[test]
fn remove_takes_only_what_it_wrote_and_the_folder_when_it_is_then_empty() {
    let r = roots("remove");
    install(&r, &bin(), 1).unwrap();
    assert_eq!(remove(&r), Ok(true));
    assert!(!r.plugin_dir().exists());
    assert_eq!(remove(&r), Ok(false));
    install(&r, &bin(), 1).unwrap();
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
    assert!(install(&r, &bin(), 1).is_err());
    assert!(std::fs::read_dir(&target).unwrap().next().is_none());
}

#[test]
fn nothing_is_written_while_antigravity_has_no_config_folder() {
    let home = std::env::temp_dir().join(format!("nl-plugin-nocfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let r = Roots::under(&home);
    assert!(install(&r, &bin(), 1).is_err());
    assert!(!home.exists());
}

fn backups(r: &Roots) -> Vec<PathBuf> {
    std::fs::read_dir(&r.config)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains(".notchling-backup-"))
        .collect()
}

#[test]
fn the_mark_says_notchling_rewrites_these_files_and_that_is_what_install_does() {
    assert!(MARK_TEXT.contains("manages these files"));
    assert!(!MARK_TEXT.contains("stops changing"));
    let r = roots("edited");
    install(&r, &bin(), 1).unwrap();
    assert!(backups(&r).is_empty());
    let sidecar = sidecar_dir(&r).join("sidecar.json");
    std::fs::write(&sidecar, "{\"edited\": true}").unwrap();
    std::fs::write(r.plugin_dir().join(MARK), "mine").unwrap();
    assert!(install(&r, &bin(), 9).unwrap());
    assert!(installed_for(&r, &bin()));
    assert_eq!(
        std::fs::read_to_string(r.plugin_dir().join(MARK)).unwrap(),
        MARK_TEXT
    );
    let found = backups(&r);
    assert_eq!(found.len(), 2);
    let texts: Vec<String> = found
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect();
    assert!(texts.contains(&"{\"edited\": true}".to_string()));
    assert!(texts.contains(&"mine".to_string()));
    assert!(found.iter().all(|p| p.to_string_lossy().ends_with("-9")));
    assert!(std::fs::read_dir(r.plugin_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .all(|e| !e.file_name().to_string_lossy().contains("backup")));
}

#[test]
fn an_unchanged_install_makes_no_backup() {
    let r = roots("same");
    install(&r, &bin(), 1).unwrap();
    install(&r, &bin(), 2).unwrap();
    assert!(backups(&r).is_empty());
}

#[cfg(unix)]
#[test]
fn preflight_finds_what_install_would_refuse_and_writes_nothing() {
    let r = roots("preflight");
    assert_eq!(preflight(&r, &bin()), Ok(()));
    assert!(!r.plugin_dir().exists());
    install(&r, &bin(), 1).unwrap();
    let sidecar = sidecar_dir(&r).join("sidecar.json");
    std::fs::remove_file(&sidecar).unwrap();
    std::os::unix::fs::symlink(r.config.join("elsewhere"), &sidecar).unwrap();
    assert!(preflight(&r, &bin()).is_err());
    assert!(preflight(&r, &PathBuf::from("notchling")).is_err());
}
