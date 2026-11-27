use super::resolver;
use notchling_engine::sessions::Roots;

fn temp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-searchacct-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_default_dir_follows_the_account_log_at_the_start_time() {
    let dir = temp("log");
    std::fs::write(
        dir.join("account-log.json"),
        r#"[{"at":"2026-11-01T00:00:00Z","id":"work"},{"at":"2026-11-12T00:00:00Z","id":"home"}]"#,
    )
    .unwrap();
    let roots = Roots {
        main_dir: dir.join("claude"),
        lanes_file: dir.join("lanes.json"),
    };
    let accounts = resolver(roots, dir.clone());
    let nov_10 = 1_794_304_800.0;
    assert_eq!(accounts(None, nov_10), "work");
    assert_eq!(accounts(None, nov_10 + 5.0 * 86_400.0), "home");
    assert_eq!(accounts(None, 1.0), "");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_lane_is_its_account_and_an_unknown_dir_is_empty() {
    let dir = temp("lane");
    std::fs::write(
        dir.join("lanes.json"),
        r#"[{"name":"one","path":"/lanes/one","accountID":"acct-1","email":"one@example.com"}]"#,
    )
    .unwrap();
    let roots = Roots {
        main_dir: dir.join("claude"),
        lanes_file: dir.join("lanes.json"),
    };
    let accounts = resolver(roots, dir.clone());
    assert_eq!(accounts(Some("/lanes/one"), 5.0), "acct-1");
    assert_eq!(accounts(Some("/lanes/other"), 5.0), "");
    let _ = std::fs::remove_dir_all(&dir);
}
