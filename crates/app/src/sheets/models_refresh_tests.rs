use serde_json::{json, Value};

use super::summary;

fn tool(
    id: &str,
    read: bool,
    added: &[&str],
    removed: &[&str],
    efforts: usize,
    unpriced: &[&str],
) -> Value {
    json!({
        "tool": id, "read": read, "added": added, "removed": removed,
        "efforts": vec![json!({"model": "m"}); efforts], "unpriced": unpriced,
    })
}

#[test]
fn what_changed_is_counted_across_the_tools() {
    let reply = json!({
        "tools": [
            tool("claude", false, &[], &[], 0, &[]),
            tool("codex", true, &["gpt-6.2-sol", "gpt-6.2-astra"], &[], 0, &["gpt-6.2-sol"]),
            tool("antigravity", true, &["gemini-4-neon"], &["old"], 2, &["gemini-4-neon", "x"]),
        ],
        "suggestions": ["review: a", "review-final: b"],
    });
    let (text, failed) = summary(Ok(reply));
    assert!(!failed);
    assert_eq!(
        text,
        "Model lists updated: 3 new models, 1 model gone, 2 models with new efforts. \
         3 models have no price. 2 job choices to review. Couldn't read: Claude Code."
    );
}

#[test]
fn nothing_changed_says_so_and_a_single_item_is_singular() {
    let quiet = json!({"tools": [tool("codex", true, &[], &[], 0, &[])], "suggestions": []});
    assert_eq!(
        summary(Ok(quiet)),
        ("The model lists are up to date.".to_owned(), false)
    );
    let one = json!({
        "tools": [tool("codex", true, &["a"], &[], 0, &["a"])],
        "suggestions": ["x"],
    });
    assert_eq!(
        summary(Ok(one)).0,
        "Model lists updated: 1 new model. 1 model has no price. 1 job choice to review."
    );
}

#[test]
fn a_failure_or_an_unknown_answer_is_an_error_line() {
    let (text, failed) = summary(Err("no answer".into()));
    assert!(failed);
    assert_eq!(text, "Couldn't refresh the models: no answer");
    let (text, failed) = summary(Ok(json!({"on": true})));
    assert!(failed);
    assert!(text.contains("isn't one this app reads"));
}
