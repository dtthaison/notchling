use super::*;
use crate::accounts::fake::MemoryKeychain;
use crate::sessions::testutil::TempDir;
use serde_json::json;
use std::sync::Arc;

fn store(tmp: &TempDir) -> LaneStore {
    LaneStore::new(&tmp.path("home"), &tmp.path("data"))
}

fn lane(store: &LaneStore, name: &str) -> Lane {
    Lane {
        name: name.into(),
        path: store.path_for(name),
        account_id: "acc:org".into(),
        email: "w@example.com".into(),
        folder: None,
    }
}

#[test]
fn the_keychain_name_is_claude_codes_exactly() {
    assert_eq!(
        service_for("/Users/me/.claude-lanes/work").unwrap(),
        "Claude Code-credentials-ee65a8d3"
    );
    assert_eq!(
        service_for("/tmp/x").unwrap(),
        "Claude Code-credentials-2e56aa36"
    );
    assert_ne!(
        service_for("/tmp/x/").unwrap(),
        service_for("/tmp/x").unwrap()
    );
    assert!(matches!(
        service_for("/Users/zoë/.claude-lanes/a"),
        Err(LaneError::NotAscii)
    ));
}

#[test]
fn names_come_from_the_email_and_stay_unique() {
    assert_eq!(new_name("Jane.Doe@example.com", &[]), "jane-doe");
    assert_eq!(
        new_name("jane.doe@x.com", &["jane-doe".into(), "jane-doe-2".into()]),
        "jane-doe-3"
    );
    assert_eq!(new_name("@x.com", &[]), "lane");
    assert_eq!(new_name("zoë@x.com", &[]), "zo");
    assert_eq!(new_name(&format!("{}@x", "a".repeat(40)), &[]).len(), 24);
}

#[test]
fn the_start_command_quotes_the_path() {
    let mut l = lane(&LaneStore::new(Path::new("/h"), Path::new("/d")), "w");
    assert_eq!(
        start_command(&l),
        "CLAUDE_CONFIG_DIR='/h/.claude-lanes/w' claude"
    );
    l.path = "/a b'c".into();
    assert_eq!(quote(&l.path), "'/a b'\\''c'");
}

#[test]
fn lanes_json_keeps_the_swift_keys_and_current_follows_the_variable() {
    let tmp = TempDir::new("lanes-json");
    let s = store(&tmp);
    let l = lane(&s, "work");
    s.save(std::slice::from_ref(&l)).unwrap();
    let text = std::fs::read_to_string(&s.lanes_file).unwrap();
    assert!(text.contains("\"accountID\":\"acc:org\""), "{text}");
    assert_eq!(s.all(), vec![l.clone()]);
    assert_eq!(s.current(Some(&format!("{}/", l.path))), Some(l.clone()));
    assert_eq!(s.current(Some("")), None);
    assert_eq!(s.current(None), None);
    assert_eq!(
        s.lane_for_account("acc:org").map(|l| l.name),
        Some("work".into())
    );
}

#[cfg(unix)]
#[test]
fn prepare_links_the_shared_entries_and_copies_the_main_config() {
    let tmp = TempDir::new("lanes-prepare");
    let s = store(&tmp);
    tmp.write("home/.claude/settings.json", "{\"hooks\":{}}");
    tmp.write(
        "home/.claude.json",
        "{\n  \"projects\": {\"/w\": {\"cost\": 1.50}},\n  \"oauthAccount\": {\"accountUuid\": \"main\"}\n}",
    );
    let l = lane(&s, "work");
    let done = s.prepare(&l, &json!({"accountUuid": "acc"}), 7).unwrap();
    assert_eq!(done.linked.len(), SHARED.len());
    let link = Path::new(&l.path).join("settings.json");
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        tmp.path("home/.claude/settings.json")
    );
    let config = std::fs::read_to_string(Path::new(&l.path).join(".claude.json")).unwrap();
    assert_eq!(
        config,
        "{\n  \"projects\": {\"/w\": {\"cost\": 1.50}},\n  \"oauthAccount\": {\n    \"accountUuid\": \"acc\"\n  }\n}"
    );
    assert!(std::fs::read_to_string(tmp.path("home/.claude.json"))
        .unwrap()
        .contains("main"));
    s.prepare(&l, &json!({"accountUuid": "acc2"}), 8).unwrap();
    let config = std::fs::read_to_string(Path::new(&l.path).join(".claude.json")).unwrap();
    assert!(config.contains("acc2") && config.contains("1.50"));
}

#[cfg(unix)]
#[test]
fn relink_lets_the_newer_copy_win_and_never_deletes() {
    let tmp = TempDir::new("lanes-relink");
    let s = store(&tmp);
    let l = lane(&s, "work");
    let dir = Path::new(&l.path).to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    tmp.write("home/.claude/settings.json", "main");
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    std::fs::File::options()
        .write(true)
        .open(tmp.path("home/.claude/settings.json"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    std::fs::write(dir.join("settings.json"), "lane").unwrap();
    std::fs::write(dir.join("CLAUDE.md"), "old lane").unwrap();
    std::fs::File::options()
        .write(true)
        .open(dir.join("CLAUDE.md"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    tmp.write("home/.claude/CLAUDE.md", "main md");
    std::fs::create_dir_all(dir.join("skills/x")).unwrap();
    let done = relink(&l, &s.main_dir, 42);
    assert_eq!(done.took_lane_copy, vec!["settings.json".to_string()]);
    assert_eq!(
        std::fs::read_to_string(tmp.path("home/.claude/settings.json")).unwrap(),
        "lane"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path("home/.claude/settings.json.lane-backup")).unwrap(),
        "main"
    );
    assert!(done.set_aside.contains(&"CLAUDE.md".to_string()));
    assert!(done.set_aside.contains(&"skills".to_string()));
    assert_eq!(
        std::fs::read_to_string(dir.join("CLAUDE.md.lane-old-42")).unwrap(),
        "old lane"
    );
    assert!(dir.join("skills.lane-old-42/x").is_dir());
    assert_eq!(
        std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
        "main md"
    );
    let again = relink(&l, &s.main_dir, 43);
    assert!(again.linked.is_empty() && again.set_aside.is_empty());
}

#[cfg(unix)]
#[test]
fn removing_a_lane_keeps_set_aside_copies_and_what_links_point_at() {
    let tmp = TempDir::new("lanes-remove");
    let s = store(&tmp);
    tmp.write("home/.claude/settings.json", "shared");
    let l = lane(&s, "work");
    s.prepare(&l, &json!({}), 1).unwrap();
    std::fs::write(Path::new(&l.path).join("CLAUDE.md.lane-old-5"), "kept").unwrap();
    s.remove_folder(&l, 99).unwrap();
    assert!(!Path::new(&l.path).exists());
    assert_eq!(
        std::fs::read_to_string(tmp.path("home/.claude/settings.json")).unwrap(),
        "shared"
    );
    assert_eq!(
        std::fs::read_to_string(s.kept_dir().join("work-99/CLAUDE.md.lane-old-5")).unwrap(),
        "kept"
    );
}

#[test]
fn folders_outside_the_lanes_folder_are_never_removed() {
    let tmp = TempDir::new("lanes-outside");
    let s = store(&tmp);
    std::fs::create_dir_all(tmp.path("home/.claude")).unwrap();
    for path in [
        tmp.path("home/.claude").to_string_lossy().into_owned(),
        format!("{}/../.claude", s.lanes_dir.display()),
        s.lanes_dir.to_string_lossy().into_owned(),
        format!("{}/a/b", s.lanes_dir.display()),
    ] {
        let mut l = lane(&s, "x");
        l.path = path.clone();
        assert!(
            matches!(s.remove_folder(&l, 1), Err(LaneError::OutsideLanes)),
            "{path}"
        );
    }
    assert!(tmp.path("home/.claude").is_dir());
}

#[test]
fn the_lane_login_keeps_the_rest_of_its_item() {
    let tmp = TempDir::new("lanes-login");
    let s = store(&tmp);
    let chain = Arc::new(MemoryKeychain::new());
    let keys = Keys {
        runner: chain.clone(),
        user: "me".into(),
    };
    let l = lane(&s, "work");
    let service = service_for(&l.path).unwrap();
    chain.put(
        &service,
        "me",
        r#"{"claudeAiOauth":{"accessToken":"test-old"},"mcpOAuth":{"s":1}}"#,
    );
    let serde_json::Value::Object(map) = json!({"accessToken": "test-new"}) else {
        unreachable!()
    };
    s.set_login(&keys, &l, &Login(map)).unwrap();
    assert_eq!(
        chain.get(&service, "me").as_deref(),
        Some(r#"{"claudeAiOauth":{"accessToken":"test-new"},"mcpOAuth":{"s":1}}"#)
    );
    assert_eq!(
        s.login(&keys, &l).unwrap().unwrap().str("accessToken"),
        Some("test-new")
    );
    assert!(s.remove_login(&keys, &l).unwrap());
    assert!(s.login(&keys, &l).unwrap().is_none());
}

#[test]
fn a_lane_config_that_cant_be_read_is_never_written_over() {
    let tmp = TempDir::new("lane-profile-unreadable");
    let s = store(&tmp);
    let l = lane(&s, "w");
    std::fs::create_dir_all(&l.path).unwrap();
    let config = Path::new(&l.path).join(".claude.json");
    let bytes = b"{\"projects\": \"\xff\xfe\"}".to_vec();
    std::fs::write(&config, &bytes).unwrap();
    assert!(s
        .set_profile(&l, &json!({"emailAddress": "w@example.com"}))
        .is_err());
    assert_eq!(std::fs::read(&config).unwrap(), bytes);
    std::fs::remove_file(&config).unwrap();
    s.set_profile(&l, &json!({"emailAddress": "w@example.com"}))
        .unwrap();
    assert!(std::fs::read_to_string(&config)
        .unwrap()
        .contains("w@example.com"));
}

#[cfg(unix)]
#[test]
fn a_lane_claude_code_made_its_own_config_for_skips_first_run_setup() {
    let tmp = TempDir::new("lanes-onboarding");
    let s = store(&tmp);
    tmp.write(
        "home/.claude.json",
        "{\n  \"hasCompletedOnboarding\": true,\n  \"lastOnboardingVersion\": \"2.1.310\",\n  \"oauthAccount\": {\"accountUuid\": \"main\"}\n}",
    );
    let l = lane(&s, "work");
    std::fs::create_dir_all(&l.path).unwrap();
    let config = Path::new(&l.path).join(".claude.json");
    std::fs::write(
        &config,
        "{\n  \"firstStartTime\": \"x\",\n  \"oauthAccount\": {\"accountUuid\": \"acc\"}\n}",
    )
    .unwrap();

    assert!(s.finish_onboarding(&l).unwrap());
    let text = std::fs::read_to_string(&config).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["hasCompletedOnboarding"], json!(true));
    assert_eq!(v["lastOnboardingVersion"], json!("2.1.310"));
    assert_eq!(v["firstStartTime"], json!("x"), "the rest is kept");
    assert_eq!(v["oauthAccount"]["accountUuid"], json!("acc"));
    assert!(!s.finish_onboarding(&l).unwrap());
    assert_eq!(std::fs::read_to_string(&config).unwrap(), text);

    std::fs::write(
        &config,
        "{\"hasCompletedOnboarding\": true, \"lastOnboardingVersion\": \"2.1.400\"}",
    )
    .unwrap();
    assert!(!s.finish_onboarding(&l).unwrap());
    let fresh = lane(&s, "fresh");
    assert!(!s.finish_onboarding(&fresh).unwrap());
    assert!(!Path::new(&fresh.path).join(".claude.json").exists());
}
