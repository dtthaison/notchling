//! The account in use where a session runs: a lane's, else the main login's.

use super::account::active_email;
use crate::sessions::testutil::TempDir;

const LANES: &str =
    r#"[{"name":"work","path":"/lanes/work","accountID":"acc-w","email":"lane@example.com"}]"#;

fn root(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    (dir.root().to_path_buf(), dir.path("data"))
}

#[test]
fn the_main_login_is_the_oauth_account_of_the_home_config() {
    let dir = TempDir::new("acct-main");
    // Everything else in the file is skipped; the real one is large.
    dir.write(
        ".claude.json",
        r#"{"projects":{"/w":{"cost":1.5}},"oauthAccount":{"accountUuid":"u","emailAddress":"main@example.com"},"userID":"x"}"#,
    );
    let (home, data) = root(&dir);
    assert_eq!(active_email(&home, &data, None), "main@example.com");
    // A folder that is no lane is the main login too (`CLAUDE_CONFIG_DIR=~/.claude`, or unset).
    assert_eq!(active_email(&home, &data, Some("")), "main@example.com");
    assert_eq!(
        active_email(&home, &data, Some("/not/a/lane")),
        "main@example.com"
    );
}

#[test]
fn a_session_in_a_lane_publishes_as_the_lanes_account() {
    let dir = TempDir::new("acct-lane");
    dir.write(
        ".claude.json",
        r#"{"oauthAccount":{"emailAddress":"main@example.com"}}"#,
    );
    dir.write("data/lanes.json", LANES);
    let (home, data) = root(&dir);
    assert_eq!(
        active_email(&home, &data, Some("/lanes/work")),
        "lane@example.com"
    );
    assert_eq!(active_email(&home, &data, None), "main@example.com");
}

#[test]
fn an_unknown_account_is_empty_and_never_an_error() {
    let dir = TempDir::new("acct-none");
    let (home, data) = root(&dir);
    // No config at all.
    assert_eq!(active_email(&home, &data, None), "");
    // Broken JSON, no oauthAccount, an account without an email, the wrong types.
    for text in [
        "{not json",
        "{}",
        r#"{"oauthAccount":{}}"#,
        r#"{"oauthAccount":"x"}"#,
        r#"{"oauthAccount":{"emailAddress":7}}"#,
    ] {
        dir.write(".claude.json", text);
        assert_eq!(active_email(&home, &data, None), "", "{text}");
    }
}
