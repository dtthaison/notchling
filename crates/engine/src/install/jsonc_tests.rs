use super::jsonc::*;
use super::testutil::*;
use serde_json::json;

#[test]
fn it_keeps_where_every_value_is() {
    let text = "{ \"a\": [1, \"two\", {\"b\": null}], \"c\": true }";
    let root = parse(text).unwrap();
    assert_eq!((root.start, root.end), (0, text.len()));
    let members = root.members().unwrap();
    assert_eq!(
        &text[members[0].start..members[0].value.end],
        "\"a\": [1, \"two\", {\"b\": null}]"
    );
    let items = members[0].value.items().unwrap();
    assert_eq!(&text[items[0].start..items[0].end], "1");
    assert_eq!(&text[items[1].start..items[1].end], "\"two\"");
    assert_eq!(&text[items[2].start..items[2].end], "{\"b\": null}");
    assert_eq!(&text[members[1].value.start..members[1].value.end], "true");
    assert_eq!(
        to_value(&root, text),
        json!({"a": [1, "two", {"b": null}], "c": true})
    );
}

#[test]
fn comments_trailing_commas_and_a_bom_are_read_and_kept_in_place() {
    let text = format!("\u{feff}{ODD}");
    let root = parse(&text).unwrap();
    let value = to_value(&root, &text);
    assert_eq!(value["model"], "opus");
    assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert_eq!(
        value["hooks"]["Notification"][0]["hooks"][0]["command"],
        "say done"
    );
    assert!(is_blank("\u{feff}  \n"));
    assert!(!is_blank("// only a comment"));
}

#[test]
fn strings_are_unescaped_and_numbers_keep_their_spelling_in_the_text() {
    let root = parse(USER_HOOKS).unwrap();
    let hooks = root.get("hooks").unwrap().get("PostToolUse").unwrap();
    let command = hooks.items().unwrap()[0]
        .get("hooks")
        .unwrap()
        .items()
        .unwrap()[0]
        .get("command")
        .unwrap();
    assert_eq!(command.as_str(), Some("prettier --write \"$CLAUDE_FILE\""));
    let ratio = root.get("env").unwrap().get("RATIO").unwrap();
    assert_eq!(&USER_HOOKS[ratio.start..ratio.end], "1.50");
    let swift = parse(SWIFT_INSTALLED).unwrap();
    let hook = swift
        .get("hooks")
        .unwrap()
        .get("PermissionRequest")
        .unwrap()
        .items()
        .unwrap()[0]
        .get("hooks")
        .unwrap()
        .items()
        .unwrap()[0]
        .get("command")
        .unwrap();
    assert_eq!(
        hook.as_str(),
        Some("'/Applications/Notchling.app/Contents/MacOS/Notchling' --hook")
    );
}

#[test]
fn broken_text_is_an_error_that_says_where_and_never_quotes_the_file() {
    let secret = "sk-ant-api03-SECRET";
    for text in [
        "{",
        "{\"a\": }",
        "{\"a\" 1}",
        "{\"a\": 1 \"b\": 2}",
        "[1 2]",
        "{\"a\": 1}}",
        "{\"a\": \"unterminated}",
        "{\"a\": \"bad \\q escape\"}",
        "{\"a\": 01}",
        "{\"a\": nul}",
        "{\"a\": /* open",
        "{'a': 1}",
        "{\"a\": 1} trailing",
        "",
    ] {
        let broken = text.replace("\"a\"", &format!("\"{secret}\""));
        let e = parse(&broken).unwrap_err();
        assert!(e.at <= broken.len(), "{text}");
        let said = e.to_string();
        assert!(!said.contains(secret), "{said}");
        assert!(said.contains("byte"), "{said}");
    }
}

#[test]
fn a_raw_control_character_in_a_string_is_refused() {
    assert!(parse("{\"a\": \"x\ty\"}").is_err());
    assert!(parse("{\"a\": \"x\\ty\"}").is_ok());
}

#[test]
fn deep_nesting_is_refused_not_overflowed() {
    let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    assert_eq!(parse(&deep).unwrap_err().what, "nested too deeply");
    let fine = format!("{}{}", "[".repeat(40), "]".repeat(40));
    assert!(parse(&fine).is_ok());
}

#[test]
fn a_backslash_at_the_very_end_is_an_error_not_a_panic() {
    for text in ["\"\\", "{\"a\": \"\\", "\"\\\u{e9}\""] {
        let _ = parse(text);
    }
    assert!(parse("\"a\\").is_err());
}

#[test]
fn duplicate_keys_are_read_and_counted() {
    let root = parse("{\"a\": 1, \"a\": 2}").unwrap();
    assert_eq!(root.count_key("a"), 2);
    assert_eq!(root.count_key("b"), 0);
}

#[test]
fn next_significant_skips_space_and_comments() {
    let text = "1 /* x */ // y\n  ,";
    assert_eq!(&text[next_significant(text, 1)..], ",");
}
