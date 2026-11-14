use super::toml_edit::{get_bool, remove, set_bool, TomlEditError};

const PLAIN: &str = include_str!("fixtures/codex-config-plain.toml");
const FEATURES: &str = include_str!("fixtures/codex-config-features.toml");
const TRICKY: &str = include_str!("fixtures/codex-config-tricky.toml");
const KEY: &str = "check_for_update_on_startup";
const DAEMON: &str = "daemon_auto_start";

fn without_line(text: &str, line: &str) -> String {
    let mut out = String::new();
    let mut dropped = false;
    for l in text.split_inclusive('\n') {
        if !dropped && l.trim_end() == line {
            dropped = true;
            continue;
        }
        out.push_str(l);
    }
    assert!(dropped, "{line} not found in {text}");
    out
}

#[test]
fn top_level_key_goes_before_the_first_table() {
    let out = set_bool(PLAIN, None, KEY, false).unwrap();
    assert_eq!(
        out,
        "# Codex config\nmodel = \"gpt-5\"\napproval_policy = \"on-request\"\ncheck_for_update_on_startup = false\n\n[projects.\"/home/w/work\"]\ntrust_level = \"trusted\"\n"
    );
    assert_eq!(get_bool(&out, None, KEY), Ok(Some(false)));
}

#[test]
fn top_level_key_with_no_other_key_goes_after_the_leading_comments() {
    let text = "# a\n# b\n[tui]\nx = 1\n";
    let out = set_bool(text, None, KEY, false).unwrap();
    assert_eq!(
        out,
        "# a\n# b\ncheck_for_update_on_startup = false\n\n[tui]\nx = 1\n"
    );
    assert_eq!(
        set_bool("", None, KEY, false).unwrap(),
        "check_for_update_on_startup = false\n"
    );
    assert_eq!(
        set_bool("# only a comment", None, KEY, false).unwrap(),
        "# only a comment\ncheck_for_update_on_startup = false\n"
    );
}

#[test]
fn features_key_under_its_table() {
    let out = set_bool(FEATURES, Some("features"), DAEMON, false).unwrap();
    assert_eq!(
        out,
        "model = \"gpt-5\"\n\n[features]\nweb_search_request = true\nunified_exec = false\ndaemon_auto_start = false\n\n[tui]\nnotifications = true\n"
    );
    assert_eq!(get_bool(&out, Some("features"), DAEMON), Ok(Some(false)));
    assert_eq!(get_bool(&out, None, DAEMON), Ok(None));
}

#[test]
fn features_table_made_when_missing() {
    let out = set_bool(PLAIN, Some("features"), DAEMON, false).unwrap();
    assert_eq!(
        out,
        format!("{PLAIN}\n[features]\ndaemon_auto_start = false\n")
    );
    let bare = set_bool("", Some("features"), DAEMON, false).unwrap();
    assert_eq!(bare, "[features]\ndaemon_auto_start = false\n");
    let no_newline = set_bool("model = \"x\"", Some("features"), DAEMON, false).unwrap();
    assert_eq!(
        no_newline,
        "model = \"x\"\n\n[features]\ndaemon_auto_start = false\n"
    );
}

#[test]
fn comments_and_order_kept() {
    let top = set_bool(PLAIN, None, KEY, false).unwrap();
    assert_eq!(
        without_line(&top, "check_for_update_on_startup = false"),
        PLAIN
    );
    let feat = set_bool(FEATURES, Some("features"), DAEMON, false).unwrap();
    assert_eq!(without_line(&feat, "daemon_auto_start = false"), FEATURES);
    let tricky = set_bool(TRICKY, None, KEY, false).unwrap();
    assert_eq!(
        without_line(&tricky, "check_for_update_on_startup = false"),
        TRICKY
    );
}

#[test]
fn existing_value_replaced_in_place() {
    let text = "a = 1\ncheck_for_update_on_startup = true # why\nb = 2\n";
    let out = set_bool(text, None, KEY, false).unwrap();
    assert_eq!(
        out,
        "a = 1\ncheck_for_update_on_startup = false # why\nb = 2\n"
    );
    let dotted = "features.daemon_auto_start = true\n";
    assert_eq!(
        set_bool(dotted, Some("features"), DAEMON, false).unwrap(),
        "features.daemon_auto_start = false\n"
    );
}

#[test]
fn already_set_is_unchanged_text() {
    let text = "check_for_update_on_startup = false\n";
    assert_eq!(set_bool(text, None, KEY, false).unwrap(), text);
    let odd = "check_for_update_on_startup   =   false   # keep\n";
    assert_eq!(set_bool(odd, None, KEY, false).unwrap(), odd);
}

#[test]
fn duplicate_key_refuses() {
    let text = "check_for_update_on_startup = true\ncheck_for_update_on_startup = false\n";
    assert_eq!(
        set_bool(text, None, KEY, false),
        Err(TomlEditError::Duplicate)
    );
    assert_eq!(get_bool(text, None, KEY), Err(TomlEditError::Duplicate));
    assert_eq!(remove(text, None, KEY), Err(TomlEditError::Duplicate));
    let tables = "[features]\na = 1\n[features]\nb = 2\n";
    assert_eq!(
        set_bool(tables, Some("features"), DAEMON, false),
        Err(TomlEditError::Duplicate)
    );
}

#[test]
fn not_a_bool_refuses() {
    let text = "check_for_update_on_startup = \"yes\"\n";
    assert_eq!(
        set_bool(text, None, KEY, false),
        Err(TomlEditError::NotABool)
    );
    assert_eq!(get_bool(text, None, KEY), Err(TomlEditError::NotABool));
}

#[test]
fn hash_inside_a_string_is_not_a_comment() {
    assert_eq!(get_bool(TRICKY, None, KEY), Ok(None));
    let text = "note = \"a # b\" # real\ncheck_for_update_on_startup = true\n";
    assert_eq!(get_bool(text, None, KEY), Ok(Some(true)));
    let out = set_bool(text, None, KEY, false).unwrap();
    assert_eq!(
        out,
        "note = \"a # b\" # real\ncheck_for_update_on_startup = false\n"
    );
}

#[test]
fn multiline_array_lines_are_not_keys() {
    assert_eq!(get_bool(TRICKY, None, DAEMON), Ok(None));
    assert_eq!(get_bool(TRICKY, Some("features"), DAEMON), Ok(None));
    let out = set_bool(TRICKY, None, KEY, false).unwrap();
    assert!(out.contains(
        "[features]\n\"\"\"\ncheck_for_update_on_startup = false\n\n[features]\na = 1\n"
    ));
}

#[test]
fn remove_takes_one_line() {
    let text = "a = 1\ncheck_for_update_on_startup = false\nb = 2\n";
    assert_eq!(remove(text, None, KEY).unwrap(), "a = 1\nb = 2\n");
    assert_eq!(remove("a = 1\n", None, KEY).unwrap(), "a = 1\n");
    let feat = set_bool(FEATURES, Some("features"), DAEMON, false).unwrap();
    assert_eq!(remove(&feat, Some("features"), DAEMON).unwrap(), FEATURES);
    let last = "a = 1\ncheck_for_update_on_startup = false";
    assert_eq!(remove(last, None, KEY).unwrap(), "a = 1\n");
}

#[test]
fn crlf_text_gets_crlf_lines() {
    let text = "model = \"x\"\r\n\r\n[tui]\r\nn = 1\r\n";
    let out = set_bool(text, None, KEY, false).unwrap();
    assert_eq!(
        out,
        "model = \"x\"\r\ncheck_for_update_on_startup = false\r\n\r\n[tui]\r\nn = 1\r\n"
    );
}

#[test]
fn unplaceable_text_refuses() {
    for text in [
        "a = \n",
        "a b = 1\n",
        "[open\nx = 1\n",
        "a = \"unterminated\n",
        "= 1\n",
        "a = [1, 2\n",
    ] {
        assert!(
            matches!(
                set_bool(text, None, KEY, false),
                Err(TomlEditError::Parse(_))
            ),
            "{text:?}"
        );
    }
    assert!(matches!(
        set_bool("", None, "bad key", false),
        Err(TomlEditError::Parse("bad_key"))
    ));
}
