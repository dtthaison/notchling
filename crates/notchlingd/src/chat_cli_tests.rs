use crate::chat_cli::*;
use serde_json::json;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| (*w).to_string()).collect()
}

#[test]
fn open_file_takes_a_path_and_an_optional_session() {
    let args = parse(&words(&[
        "open-file",
        "/tmp/a.rb",
        "--session",
        "s1",
        "--json",
    ]))
    .unwrap();
    assert_eq!(args.path, "/tmp/a.rb");
    assert_eq!(args.session.as_deref(), Some("s1"));
    assert!(args.json);
}

#[test]
fn a_relative_path_becomes_absolute() {
    let args = parse(&words(&["open-file", "a.rb"])).unwrap();
    assert!(std::path::Path::new(&args.path).is_absolute());
    assert!(args.path.ends_with("a.rb"));
}

#[test]
fn a_missing_path_or_an_unknown_option_is_usage() {
    assert!(parse(&words(&["open-file"])).is_err());
    assert!(parse(&words(&["open-file", "/a", "--bogus"])).is_err());
    assert!(parse(&words(&["open-file", "/a", "--session"])).is_err());
    assert!(parse(&words(&["close"])).is_err());
}

#[test]
fn the_sentence_names_the_file() {
    assert_eq!(
        sentence(&json!({"ok": true, "path": "/tmp/a.rb"})),
        "Opened /tmp/a.rb in Chat."
    );
}
