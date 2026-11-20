use crate::install_ops::InstallSetup;
use crate::install_ops_tests::{data, ops_for, rig, settings_text, Dir, BINARY, USER_SETTINGS};
use notchling_engine::catalog::Tool;
use notchling_engine::cost_profile::guard;
use notchling_engine::cost_profile::manifest::{Entry, Manifest, Part};
use notchling_engine::settings::Settings;
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::path::PathBuf;

fn setup_with_data(dir: &Dir, settings: Option<&str>) -> InstallSetup {
    let mut setup = rig(dir, settings);
    setup.guard_data = Some(dir.path("data"));
    std::fs::create_dir_all(dir.path("data")).unwrap();
    setup
}

fn set_up(dir: &Dir) {
    let entry = Entry {
        id: "claude.rule".into(),
        tool: Tool::Claude,
        path: PathBuf::from("/x/notchling-cost.md"),
        part: Part::File,
        removed: false,
        hash: None,
        backup: None,
        existed: false,
        keys: Vec::new(),
        at: 1,
        pending: false,
        before_hash: None,
        released: false,
    };
    let mut manifest = Manifest::default();
    manifest.put(entry);
    manifest.save(&dir.path("data")).unwrap();
}

fn set_guard(dir: &Dir, level: &str) {
    let mut store = Settings::open(&dir.path("data/settings.json")).unwrap();
    store.set(guard::KEY, Value::from(level)).unwrap();
}

fn guard_apply() -> Value {
    json!({"what": ["guard_hooks"]})
}

fn entries(text: &str, event: &str) -> Vec<Value> {
    data(text)["hooks"][event]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn hook_of(group: &Value) -> (String, Option<u64>) {
    let hook = &group["hooks"][0];
    (
        hook["command"].as_str().unwrap_or("").to_string(),
        hook["timeout"].as_u64(),
    )
}

#[test]
fn before_set_up_the_guard_is_not_installed() {
    let dir = Dir::new("guard-before");
    let (ops, _) = ops_for(setup_with_data(&dir, Some(USER_SETTINGS)), true);
    let reply = ops.run(ops::INSTALL_APPLY, &guard_apply()).unwrap();
    assert_eq!(reply["plan"]["hooks"]["changed"], false);
    assert_eq!(settings_text(&dir), USER_SETTINGS);
}

#[test]
fn after_set_up_the_guard_goes_in_as_its_own_entries_and_the_users_hooks_stay() {
    let dir = Dir::new("guard-after");
    set_up(&dir);
    let (ops, _) = ops_for(setup_with_data(&dir, Some(USER_SETTINGS)), true);
    let reply = ops.run(ops::INSTALL_APPLY, &guard_apply()).unwrap();
    assert_eq!(reply["applied"], true);
    assert!(reply["backup"].is_string());
    let text = settings_text(&dir);
    let pre = entries(&text, "PreToolUse");
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0]["hooks"][0]["command"], "~/bin/guard-bash.sh");
    assert_eq!(pre[1]["matcher"], "Bash|Read|Agent");
    assert_eq!(
        hook_of(&pre[1]),
        (format!("'{BINARY}' hook --guard"), Some(5))
    );
    for event in ["SubagentStart", "SubagentStop"] {
        let list = entries(&text, event);
        assert_eq!(list.len(), 1, "{event}");
        assert_eq!(
            hook_of(&list[0]),
            (format!("'{BINARY}' hook --guard"), Some(5))
        );
    }
    assert_eq!(data(&text)["model"], "opus");
    assert_eq!(data(&text)["env"]["API_TOKEN"], "sk-ant-api03-NEVERSHOWN");
}

#[test]
fn the_guard_and_the_fence_are_separate_entries_with_their_own_timeouts() {
    let dir = Dir::new("guard-fence");
    set_up(&dir);
    let (ops, _) = ops_for(setup_with_data(&dir, Some(USER_SETTINGS)), true);
    ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    let pre = entries(&settings_text(&dir), "PreToolUse");
    let find = |flag: &str| -> Vec<&Value> {
        pre.iter()
            .filter(|g| hook_of(g).0.ends_with(flag))
            .collect()
    };
    let fence = find("hook --fence");
    let guard = find("hook --guard");
    assert_eq!((fence.len(), guard.len()), (1, 1));
    assert_eq!(fence[0]["matcher"], "*");
    assert_eq!(hook_of(fence[0]).1, Some(30));
    assert_eq!(guard[0]["matcher"], "Bash|Read|Agent");
    assert_eq!(hook_of(guard[0]).1, Some(5));
    let again = ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    assert_eq!(again["plan"]["hooks"]["changed"], false);
}

#[test]
fn turning_the_guard_off_takes_only_its_entries_out() {
    let dir = Dir::new("guard-off");
    set_up(&dir);
    let (ops, _) = ops_for(setup_with_data(&dir, Some(USER_SETTINGS)), true);
    ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    set_guard(&dir, "off");
    let reply = ops.run(ops::INSTALL_APPLY, &guard_apply()).unwrap();
    assert_eq!(reply["applied"], true);
    let text = settings_text(&dir);
    assert!(!text.contains("--guard"));
    assert!(text.contains("hook --fence"));
    assert!(text.contains("~/bin/guard-bash.sh"));
    assert!(data(&text)["hooks"]["SubagentStart"].is_null());
}

#[test]
fn a_full_install_with_the_guard_off_leaves_it_out() {
    let dir = Dir::new("guard-off-full");
    set_up(&dir);
    set_guard(&dir, "off");
    let (ops, _) = ops_for(setup_with_data(&dir, Some(USER_SETTINGS)), true);
    ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    assert!(!settings_text(&dir).contains("--guard"));
}

#[test]
fn removing_every_hook_removes_the_guard_too() {
    let dir = Dir::new("guard-remove");
    set_up(&dir);
    let (ops, _) = ops_for(setup_with_data(&dir, Some(USER_SETTINGS)), true);
    ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    ops.run(ops::INSTALL_REMOVE, &json!({})).unwrap();
    assert!(!settings_text(&dir).contains("--guard"));
    assert!(settings_text(&dir).contains("~/bin/guard-bash.sh"));
}

#[test]
fn with_the_switch_off_the_guard_writes_nothing() {
    let dir = Dir::new("guard-switch");
    set_up(&dir);
    let (ops, _) = ops_for(setup_with_data(&dir, Some(USER_SETTINGS)), false);
    let reply = ops.run(ops::INSTALL_APPLY, &guard_apply()).unwrap();
    assert_eq!(reply["applied"], false);
    assert_eq!(settings_text(&dir), USER_SETTINGS);
}

#[test]
fn the_guard_needs_a_set_up_claude_and_a_level_that_is_not_off() {
    let dir = Dir::new("guard-gate");
    let data_dir = dir.path("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    assert!(!guard::enabled(&data_dir));
    assert_eq!(guard::level(&data_dir), "log");
    set_up(&dir);
    assert!(guard::enabled(&data_dir));
    set_guard(&dir, "off");
    assert!(!guard::enabled(&data_dir));
    set_guard(&dir, "log");
    assert!(guard::enabled(&data_dir));
    let mut store = Settings::open(&dir.path("data/settings.json")).unwrap();
    store.set("costClaude", Value::from(false)).unwrap();
    assert!(!guard::enabled(&data_dir));
}

#[test]
fn syncing_writes_and_clears_the_policy_the_hook_reads() {
    let dir = Dir::new("guard-policy");
    let data_dir = dir.path("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let policy = data_dir.join("guard/policy.json");
    assert!(!guard::sync(&data_dir, 5));
    assert!(!policy.exists());
    set_up(&dir);
    assert!(guard::sync(&data_dir, 7));
    let text: Value = serde_json::from_str(&std::fs::read_to_string(&policy).unwrap()).unwrap();
    assert_eq!(
        (text["level"].clone(), text["cap"].clone()),
        (json!("log"), json!(7))
    );
    set_guard(&dir, "off");
    assert!(!guard::sync(&data_dir, 7));
    assert!(!policy.exists());
}

#[test]
fn installing_the_guard_for_a_set_up_made_before_also_writes_the_policy() {
    let dir = Dir::new("guard-policy-install");
    set_up(&dir);
    let policy = dir.path("data/guard/policy.json");
    assert!(!policy.exists());
    let (ops, _) = ops_for(setup_with_data(&dir, Some(USER_SETTINGS)), true);
    ops.run(ops::INSTALL_APPLY, &guard_apply()).unwrap();
    assert!(policy.exists());
}
