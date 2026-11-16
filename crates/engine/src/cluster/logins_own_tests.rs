use super::claude_profile_signed_in;

#[test]
fn an_account_profile_is_a_login() {
    assert!(claude_profile_signed_in(
        r#"{"numStartups": 3, "oauthAccount": {"accountUuid": "u-1", "emailAddress": "a@example.com"}, "tipsHistory": {}}"#
    ));
    assert!(claude_profile_signed_in(
        r#"{"oauthAccount": {"accountUuid": "u-1"}}"#
    ));
}

#[test]
fn no_profile_is_no_login() {
    for text in [
        "",
        "{}",
        r#"{"numStartups": 3}"#,
        r#"{"oauthAccount": null}"#,
        r#"{"oauthAccount": {}}"#,
        r#"{"oauthAccount": "x"}"#,
        r#"{"oauthAccount": 1}"#,
        r#"{"oauthAccount": []}"#,
        r#"["oauthAccount"]"#,
        "not json",
        r#"{"oauthAccount": {"accountUuid": "u-1""#,
    ] {
        assert!(!claude_profile_signed_in(text), "{text:?}");
    }
}

#[test]
fn only_the_top_level_key_counts() {
    assert!(!claude_profile_signed_in(
        r#"{"projects": {"/w": {"oauthAccount": {"accountUuid": "u-1"}}}}"#
    ));
}
