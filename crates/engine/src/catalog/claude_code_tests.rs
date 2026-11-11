use super::claude_code::*;
use crate::sessions::testutil::TempDir;

/// The shape seen in a real `~/.claude.json`, with other keys around it.
const CONFIG: &str = r#"{
  "numStartups": 3,
  "oauthAccount": {"emailAddress": "someone@example.com"},
  "additionalModelOptionsCache": [
    {"value": "claude-fable-5-1[1m]", "label": "Fable", "description": "Fable 5.1 · Most capable for complex work"},
    {"value": "claude-fable-5-1", "label": "Fable", "description": "Fable 5.1 · again, bare"},
    {"value": "claude-sonnet-6", "label": "Sonnet", "description": "Best for everyday tasks"},
    {"value": "claude-haiku-6", "label": "Haiku"},
    {"value": "opus", "label": "Opus alias"},
    {"value": "claude-opus-5-5 --flag", "label": "Bad"},
    {"value": "Claude-Opus-9", "label": "Bad case"},
    {"label": "No value"},
    {"value": 5}
  ]
}"#;

#[test]
fn reads_claude_code_s_own_list_by_bare_id_with_its_names() {
    let models = parse(CONFIG.as_bytes());
    let got: Vec<(&str, Option<&str>)> = models
        .iter()
        .map(|m| (m.id.as_str(), m.name.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("claude-fable-5-1", Some("Fable 5.1")),
            ("claude-sonnet-6", Some("Sonnet")),
            ("claude-haiku-6", Some("Haiku")),
        ]
    );
    for m in &models {
        assert_eq!(m.context_window, None);
        assert!(m.efforts.is_empty());
    }
}

#[test]
fn a_missing_file_or_key_or_garbage_is_no_models() {
    let tmp = TempDir::new("claude-code-options");
    assert!(read(&tmp.path("missing.json")).is_empty());
    assert!(read(&tmp.write("none.json", r#"{"numStartups": 1}"#)).is_empty());
    assert!(read(&tmp.write("broken.json", "{not json")).is_empty());
    assert!(read(&tmp.write("wrong.json", r#"{"additionalModelOptionsCache": 3}"#)).is_empty());
    let file = tmp.write("config.json", CONFIG);
    let before = std::fs::read(&file).unwrap();
    assert_eq!(read(&file).len(), 3);
    assert_eq!(std::fs::read(&file).unwrap(), before, "read only");
}
