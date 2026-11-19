use super::*;

fn plugin_dir(r: &Rigged) -> PathBuf {
    r.roots.plugin_dir()
}

fn seed_config(r: &Rigged) -> PathBuf {
    std::fs::create_dir_all(&r.roots.config).unwrap();
    let path = r.roots.config_json();
    std::fs::write(&path, CONFIG).unwrap();
    path
}

fn setup_args(action: &str, confirm: bool) -> Value {
    json!({"action": action, "confirm": confirm})
}

#[test]
fn turning_on_needs_a_confirmation_and_the_install_switch_and_writes_nothing_without_them() {
    let r = rigged(Some("sidecar"), true, true);
    let config = seed_config(&r);
    let (kind, message) = code(r.ops.call(
        &app(),
        ops::AGY_SIDECAR_SETUP,
        &setup_args("turn_on", false),
    ));
    assert_eq!(kind, ErrorCode::BadRequest, "{message}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), CONFIG);
    assert!(!plugin_dir(&r).exists());
    assert!(!r.roots.hooks_json().exists());

    let off = rigged(Some("sidecar"), false, true);
    let config = seed_config(&off);
    let (kind, _) = code(off.ops.call(
        &app(),
        ops::AGY_SIDECAR_SETUP,
        &setup_args("turn_on", true),
    ));
    assert_eq!(kind, ErrorCode::Forbidden);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), CONFIG);
    assert!(!plugin_dir(&off).exists());

    let gone = rigged(Some("sidecar"), true, false);
    let config = seed_config(&gone);
    let (kind, message) = code(gone.ops.call(
        &app(),
        ops::AGY_SIDECAR_SETUP,
        &setup_args("turn_on", true),
    ));
    assert_eq!(kind, ErrorCode::Failed);
    assert!(message.contains("not installed"), "{message}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), CONFIG);
}

#[test]
fn turning_on_sets_only_the_one_config_key_backs_it_up_and_ships_the_plugin_and_hooks() {
    let r = rigged(Some("sidecar"), true, true);
    let config = seed_config(&r);
    let out = r
        .ops
        .call(&app(), ops::AGY_SIDECAR_SETUP, &setup_args("turn_on", true))
        .unwrap();
    assert_eq!(out["state"], "on");
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("12345678901234567890"));
    let mut before: Value = serde_json::from_str(CONFIG).unwrap();
    let after: Value = serde_json::from_str(&text).unwrap();
    before["sidecars"][SIDECAR_ID] = json!({"enabled": true});
    assert_eq!(after, before);
    let backups = out["backups"].as_array().unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read_to_string(backups[0].as_str().unwrap()).unwrap(),
        CONFIG
    );
    let sidecar_json: Value = serde_json::from_str(
        &std::fs::read_to_string(
            plugin_dir(&r)
                .join("sidecars")
                .join(SIDECAR_FOLDER)
                .join("sidecar.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(sidecar_json["command"], BINARY);
    assert_eq!(sidecar_json["args"], json!(["agy-sidecar"]));
    let hooks = std::fs::read_to_string(r.roots.hooks_json()).unwrap();
    assert!(hooks.contains("agy-hook pre-tool"));
    assert!(hooks.contains("agy-hook pre-invocation"));
    assert!(plugin_dir(&r).ends_with(format!("plugins/{PLUGIN}")));

    let again = r
        .ops
        .call(&app(), ops::AGY_SIDECAR_SETUP, &setup_args("turn_on", true))
        .unwrap();
    assert_eq!(again["state"], "on");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), text);
}

#[test]
fn turning_off_takes_notchlings_own_files_and_its_one_config_key_and_the_state_reads_off() {
    let r = rigged(Some("sidecar"), true, true);
    let config = seed_config(&r);
    std::fs::write(
        r.roots.hooks_json(),
        "{\n  \"mine\": {\"PreToolUse\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"echo hi\"}]}]}\n}\n",
    )
    .unwrap();
    r.ops
        .call(&app(), ops::AGY_SIDECAR_SETUP, &setup_args("turn_on", true))
        .unwrap();
    assert_eq!(r.ops.status()["state"], "on");
    let out = r
        .ops
        .call(
            &app(),
            ops::AGY_SIDECAR_SETUP,
            &setup_args("turn_off", false),
        )
        .unwrap();
    assert_eq!(out["plugin"], "absent");
    assert_eq!(out["hooks"], "absent");
    assert_eq!(out["enabled"], "unset");
    assert_eq!(out["state"], "off");
    assert_eq!(out["backups"].as_array().unwrap().len(), 2);
    assert!(!plugin_dir(&r).exists());
    let hooks = std::fs::read_to_string(r.roots.hooks_json()).unwrap();
    assert!(hooks.contains("echo hi"));
    assert!(!hooks.contains("agy-hook"));
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("12345678901234567890"));
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        serde_json::from_str::<Value>(CONFIG).unwrap()
    );
    assert_eq!(r.ops.status()["state"], "off");
}

#[test]
fn turning_on_and_off_a_config_with_no_sidecars_gives_the_file_back_byte_for_byte() {
    let r = rigged(Some("sidecar"), true, true);
    std::fs::create_dir_all(&r.roots.config).unwrap();
    let plain = "{\n  \"plugins\": {},\n  \"big\": 12345678901234567890\n}\n";
    std::fs::write(r.roots.config_json(), plain).unwrap();
    r.ops
        .call(&app(), ops::AGY_SIDECAR_SETUP, &setup_args("turn_on", true))
        .unwrap();
    assert_ne!(
        std::fs::read_to_string(r.roots.config_json()).unwrap(),
        plain
    );
    r.ops
        .call(
            &app(),
            ops::AGY_SIDECAR_SETUP,
            &setup_args("turn_off", false),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(r.roots.config_json()).unwrap(),
        plain
    );
    assert_eq!(r.ops.status()["state"], "off");
}

#[test]
fn a_config_turning_on_would_refuse_stops_it_before_the_plugin_or_the_hooks_are_written() {
    for bad in ["{ nope", "[]", "", "{\"sidecars\": 3}"] {
        let r = rigged(Some("sidecar"), true, true);
        std::fs::create_dir_all(&r.roots.config).unwrap();
        std::fs::write(r.roots.config_json(), bad).unwrap();
        let (kind, _) = code(r.ops.call(
            &app(),
            ops::AGY_SIDECAR_SETUP,
            &setup_args("turn_on", true),
        ));
        assert_eq!(kind, ErrorCode::Failed, "{bad}");
        assert!(!plugin_dir(&r).exists(), "{bad}");
        assert!(!r.roots.hooks_json().exists(), "{bad}");
        assert_eq!(std::fs::read_to_string(r.roots.config_json()).unwrap(), bad);
    }
    let r = rigged(Some("sidecar"), true, true);
    std::fs::create_dir_all(&r.roots.config).unwrap();
    let (kind, message) = code(r.ops.call(
        &app(),
        ops::AGY_SIDECAR_SETUP,
        &setup_args("turn_on", true),
    ));
    assert_eq!(kind, ErrorCode::Failed);
    assert!(message.contains("config.json"), "{message}");
    assert!(!plugin_dir(&r).exists());
    assert!(!r.roots.hooks_json().exists());
}

#[test]
fn a_read_only_config_or_a_hooks_file_that_is_a_folder_stops_turning_on_before_any_write() {
    use std::os::unix::fs::PermissionsExt;
    let r = rigged(Some("sidecar"), true, true);
    let config = seed_config(&r);
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o444)).unwrap();
    let (kind, message) = code(r.ops.call(
        &app(),
        ops::AGY_SIDECAR_SETUP,
        &setup_args("turn_on", true),
    ));
    assert_eq!(kind, ErrorCode::Failed);
    assert!(message.contains("read-only"), "{message}");
    assert!(!plugin_dir(&r).exists());
    assert!(!r.roots.hooks_json().exists());
    assert_eq!(std::fs::read_to_string(&config).unwrap(), CONFIG);

    let r = rigged(Some("sidecar"), true, true);
    let config = seed_config(&r);
    std::fs::create_dir(r.roots.hooks_json()).unwrap();
    let (kind, _) = code(
        r.ops
            .call(&app(), ops::AGY_SIDECAR_SETUP, &setup_args("turn_on", true)),
    );
    assert_eq!(kind, ErrorCode::Failed);
    assert!(!plugin_dir(&r).exists());
    assert_eq!(std::fs::read_to_string(&config).unwrap(), CONFIG);
}

#[test]
fn a_malformed_hooks_file_stops_turning_on_before_the_config_is_touched() {
    let r = rigged(Some("sidecar"), true, true);
    let config = seed_config(&r);
    std::fs::write(r.roots.hooks_json(), "{ not json").unwrap();
    let (kind, _) = code(
        r.ops
            .call(&app(), ops::AGY_SIDECAR_SETUP, &setup_args("turn_on", true)),
    );
    assert_eq!(kind, ErrorCode::Failed);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), CONFIG);
    assert!(!plugin_dir(&r).exists());
    assert_eq!(
        std::fs::read_to_string(r.roots.hooks_json()).unwrap(),
        "{ not json"
    );
}

#[test]
fn the_state_row_says_off_on_or_no_app() {
    let r = rigged(Some("sidecar"), true, true);
    seed_config(&r);
    let state = r.ops.status();
    assert_eq!(state["state"], "off");
    assert_eq!(state["chosen"], true);
    assert_eq!(state["enabled"], "unset");
    assert_eq!(state["running"], false);
    r.ops
        .call(&app(), ops::AGY_SIDECAR_SETUP, &setup_args("turn_on", true))
        .unwrap();
    assert_eq!(r.ops.status()["state"], "on");
    let none = rigged(None, true, false);
    assert_eq!(none.ops.status()["state"], "no_app");
    assert_eq!(none.ops.status()["chosen"], false);
}

#[test]
fn a_foreign_plugin_folder_blocks_and_is_never_touched() {
    let r = rigged(Some("sidecar"), true, true);
    seed_config(&r);
    std::fs::create_dir_all(plugin_dir(&r)).unwrap();
    std::fs::write(plugin_dir(&r).join("plugin.json"), "{}").unwrap();
    assert_eq!(r.ops.status()["state"], "blocked");
    let (kind, _) = code(
        r.ops
            .call(&app(), ops::AGY_SIDECAR_SETUP, &setup_args("turn_on", true)),
    );
    assert_eq!(kind, ErrorCode::Failed);
    assert_eq!(
        std::fs::read_to_string(plugin_dir(&r).join("plugin.json")).unwrap(),
        "{}"
    );
    assert_eq!(
        std::fs::read_to_string(r.roots.config_json()).unwrap(),
        CONFIG
    );
}
