use notchling_locale as t;
use serde_json::json;

use super::models_rows::*;

fn found(reply: serde_json::Value) -> (Vec<RoleLine>, OverrideInfo) {
    match ModelsShown::from_reply(Ok(reply), "/Users/me") {
        ModelsShown::Found { roles, file } => (roles, file),
        other => panic!("{other:?}"),
    }
}

#[test]
fn each_job_lists_its_choices_with_tool_effort_source_and_notes() {
    let (roles, _) = found(json!({"roles": [
        {"role": "review", "choices": [
            {"tool": "antigravity", "marked": "gemini-4-hydrogen", "model": "gemini-4.1-hydrogen",
             "effort": "high", "source": "live", "note": "gemini-4-hydrogen is gone; using gemini-4.1-hydrogen (same family, newer)",
             "advice": "Review after each task.", "checked": true}]},
        {"role": "review-final", "choices": [
            {"tool": "codex", "marked": "gpt-6-astra", "model": "gpt-6-astra", "effort": "xhigh",
             "source": "built_in", "note": null, "advice": null, "checked": false}]},
        {"role": "spec-check", "choices": []}],
        "override": {"path": null, "state": "missing"}}));
    assert_eq!(
        roles.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["Review", "Final Review", "Spec Check"]
    );
    let review = &roles[0].choices[0];
    assert_eq!(review.model, "gemini-4.1-hydrogen");
    assert_eq!(review.tool_effort, "Antigravity · effort high");
    assert_eq!(review.source, Source::Live);
    assert!(review.note.as_deref().unwrap().contains("is gone"));
    assert_eq!(review.advice.as_deref(), Some("Review after each task."));
    assert!(!review.unchecked && review.has_model);
    let last = &roles[1].choices[0];
    assert_eq!(last.tool_effort, "Codex · effort xhigh");
    assert_eq!(last.source, Source::BuiltIn);
    assert!(last.unchecked && last.note.is_none());
    assert!(roles[2].choices.is_empty());
}

#[test]
fn a_job_with_no_live_model_says_so_and_an_unknown_job_keeps_its_name() {
    let (roles, _) = found(json!({"roles": [
        {"role": "triage", "choices": [
            {"tool": "codex", "marked": "gpt-7", "model": null, "effort": "low",
             "source": "override", "checked": true}]}],
        "override": {"path": null, "state": "missing"}}));
    assert_eq!(roles[0].title, "triage");
    let c = &roles[0].choices[0];
    assert!(!c.has_model);
    assert_eq!(c.model, t::settings::model_none());
    assert_eq!(c.source, Source::Override);
    assert_eq!(Source::Override.word(), "Your override");
    assert_eq!(Source::BuiltIn.word(), "Built-in");
    assert_eq!(Source::Live.word(), "Live list");
}

#[test]
fn the_override_file_shows_with_the_home_folder_as_a_tilde() {
    let (_, file) = found(json!({"roles": [], "override": {
        "path": "/Users/me/Library/Application Support/Notchling/catalog.json",
        "state": "found"}}));
    assert_eq!(
        file.path.as_deref(),
        Some("/Users/me/Library/Application Support/Notchling/catalog.json")
    );
    assert_eq!(
        file.shown,
        "~/Library/Application Support/Notchling/catalog.json"
    );
    assert_eq!(file.state, OverrideState::Found);
    assert_eq!(file.state.line(), t::settings::override_found());
    for (word, state) in [
        ("missing", OverrideState::Missing),
        ("unreadable", OverrideState::Unreadable),
    ] {
        let (_, file) = found(json!({"roles": [], "override": {"path": "/x", "state": word}}));
        assert_eq!(file.state, state);
    }
}

#[test]
fn failures_and_a_missing_daemon_are_lines() {
    assert_eq!(
        ModelsShown::from_reply(Err(crate::acting::DETACHED_MESSAGE.to_owned()), "/h"),
        ModelsShown::Detached
    );
    assert_eq!(
        ModelsShown::from_reply(Err("boom".into()), "/h"),
        ModelsShown::Failed("boom".into())
    );
    assert_eq!(
        ModelsShown::from_reply(Ok(json!({})), "/h"),
        ModelsShown::Failed(t::settings::reply_unreadable().to_owned())
    );
    assert_eq!(
        ModelsShown::Checking.status().as_deref(),
        Some(t::settings::models_checking())
    );
    assert!(ModelsShown::Checking.file().is_none());
}
