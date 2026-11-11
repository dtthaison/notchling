use super::claude_live::*;
use crate::accounts::fake::ScriptedHttp;
use crate::accounts::Secret;

const FIXTURE: &str = include_str!("../../tests/fixtures/catalog/models-list.json");

#[test]
fn parses_ids_and_names_and_ignores_the_api_window() {
    let models = parse(FIXTURE.as_bytes());
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "claude-fable-5-3");
    assert_eq!(models[0].name.as_deref(), Some("Claude Fable 5.3"));
    assert_eq!(models[1].id, "claude-opus-5-5");
    for m in &models {
        assert_eq!(m.context_window, None, "max_input_tokens is not the window");
        assert!(m.efforts.is_empty());
    }
}

#[test]
fn garbage_and_missing_ids_parse_to_nothing_or_skip() {
    assert!(parse(b"<html>").is_empty());
    assert!(parse(b"{\"data\": 3}").is_empty());
    let models =
        parse(br#"{"data": [{"display_name": "No id"}, {"id": ""}, {"id": "claude-x-1"}]}"#);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "claude-x-1");
    assert_eq!(models[0].name, None);
}

#[test]
fn a_good_answer_is_read_with_the_version_header_and_the_bearer() {
    let http = ScriptedHttp::new();
    http.answer(MODELS_URL, 200, FIXTURE);
    let models = fetch(&http, &Secret::new("test-token")).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(
        http.seen(),
        vec![(MODELS_URL.to_string(), Some("test-token".to_string()))]
    );
    assert_eq!(
        http.seen_headers()[0],
        vec![("anthropic-version".to_string(), "2023-06-01".to_string())]
    );
}

#[test]
fn a_401_or_a_500_or_garbage_is_an_error_and_nothing_else_is_sent() {
    for (status, body) in [
        (401, "{\"error\": \"test-token\"}"),
        (500, "oops"),
        (200, "<html>"),
    ] {
        let http = ScriptedHttp::new();
        http.answer(MODELS_URL, status, body);
        let err = fetch(&http, &Secret::new("test-token")).unwrap_err();
        assert_eq!(err.refused(), status == 401, "{err}");
        let err = err.to_string();
        assert!(!err.contains("test-token") && !err.contains("html") && !err.contains("oops"));
        assert_eq!(http.seen().len(), 1, "one call, no retry");
        assert_eq!(http.seen()[0].1.as_deref(), Some("test-token"));
        assert!(http.seen_headers()[0]
            .contains(&("anthropic-version".to_string(), "2023-06-01".to_string())));
    }
    // No answer at all (status 0) is an error too.
    let http = ScriptedHttp::new();
    assert!(fetch(&http, &Secret::new("t")).is_err());
}
