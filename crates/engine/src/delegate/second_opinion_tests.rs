use super::*;
use crate::delegate::options::parse;
use crate::delegate::outcome::{to_json, Outcome};
use std::path::Path;

fn diff() -> Vec<DiffLine> {
    let line = |kind, text: &str, number| DiffLine {
        kind,
        text: text.into(),
        number,
    };
    vec![
        line(DiffKind::Hunk, "@@ -1,2 +1,2 @@", None),
        line(DiffKind::Context, "let a = 1", Some(1)),
        line(DiffKind::Remove, "let b = 2", Some(2)),
        line(DiffKind::Add, "let b = 3", Some(2)),
    ]
}

fn change() -> Subject {
    Subject::Change {
        path: "/w/App/Foo.swift".into(),
        diff: diff(),
    }
}

fn opinion(tool: &str) -> SecondOpinion {
    SecondOpinion {
        id: "id".into(),
        session: "s".into(),
        anchor: "a".into(),
        subject: "Reply".into(),
        tool: tool.into(),
        model: None,
        effort: None,
        folder: "/w".into(),
        started: 1.0,
        status: Status::Running,
        reply: String::new(),
        error: None,
        delegated_session: None,
        seconds: 0.0,
        tokens: None,
    }
}

#[test]
fn a_change_prompt_has_the_folder_the_file_and_a_unified_diff() {
    let p = build_prompt(&change(), "/w");
    assert!(p.starts_with("Review this change for bugs"));
    assert!(p.contains("Folder: /w"));
    assert!(p.contains("File: /w/App/Foo.swift"));
    assert!(p.contains("```diff\n@@ -1,2 +1,2 @@\n let a = 1\n-let b = 2\n+let b = 3\n```"));
    assert_eq!(title(&change()), "Foo.swift");
    let nameless = Subject::Change {
        path: String::new(),
        diff: vec![],
    };
    assert!(!build_prompt(&nameless, "/w").contains("File:"));
    assert_eq!(title(&nameless), "Change");
}

#[test]
fn a_message_prompt_is_trimmed_and_a_long_one_is_clipped() {
    let p = build_prompt(&Subject::Message("  The cache is safe.  ".into()), "/w");
    assert!(p.contains("Folder: /w"));
    assert!(p.ends_with("Message:\n\nThe cache is safe."));
    let long = "x".repeat(BODY_LIMIT + 50);
    let clipped = build_prompt(&Subject::Message(long), "/w");
    assert!(clipped.contains("(50 more characters left out)"));
    assert_eq!(title(&Subject::Message("x".into())), "Reply");
    // Counted in characters, not bytes.
    assert_eq!(clip(&"é".repeat(BODY_LIMIT)).chars().count(), BODY_LIMIT);
}

#[test]
fn the_arguments_are_read_only_and_skip_unset_choices() {
    let a = SecondOpinion::arguments(Tool::Codex, None, Some(""), "/w", "/tmp/p.txt", 900);
    assert_eq!(
        a,
        [
            "delegate",
            "codex",
            "--folder",
            "/w",
            "--timeout",
            "900",
            "--json",
            "--prompt-file",
            "/tmp/p.txt"
        ]
    );
    let dir = std::env::temp_dir().join(format!("nl-so-args-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prompt.txt");
    std::fs::write(&file, "from file").unwrap();
    let b = SecondOpinion::arguments(
        Tool::Agy,
        Some("gemini-4-argon"),
        Some("high"),
        "/w",
        file.to_str().unwrap(),
        900,
    );
    assert!(!b.contains(&"--write".to_string()));
    assert_eq!(
        b[..6],
        [
            "delegate",
            "agy",
            "--model",
            "gemini-4-argon",
            "--effort",
            "high"
        ]
    );
    // The command accepts them.
    let parsed = parse(&b[1..], Path::new("/"), None).unwrap();
    assert_eq!(parsed.options.tool, Tool::Agy);
    assert_eq!(parsed.options.prompt, "from file");
    assert!(parsed.json && !parsed.options.write);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn it_reads_the_delegate_json_back() {
    let mut o = Outcome::new(Tool::Codex);
    o.session = Some("abc".into());
    o.model = Some("gpt-x".into());
    o.effort = Some("high".into());
    o.reply = "Line 3 looks off.".into();
    o.seconds = 12.0;
    o.tokens = Some(900);
    let r = Answer::parse(&format!("noise\n{}\n", to_json(&o))).unwrap();
    assert!(r.ok);
    assert_eq!(r.reply, "Line 3 looks off.");
    assert_eq!(r.session.as_deref(), Some("abc"));
    assert_eq!(r.model.as_deref(), Some("gpt-x"));
    assert_eq!((r.seconds, r.tokens), (12.0, Some(900)));
    assert_eq!(Answer::parse("Error: no folder"), None);
    assert_eq!(Answer::parse("{\"ok\":true}"), None, "no tool: not ours");
    assert_eq!(Answer::parse("{broken"), None);

    o.error = Some("boom".into());
    o.timed_out = true;
    let failed = Answer::parse(&to_json(&o)).unwrap();
    assert!(!failed.ok && failed.timed_out);
    assert_eq!(failed.error.as_deref(), Some("boom"));
}

#[test]
fn finish_applies_the_result() {
    let mut o = Outcome::new(Tool::Agy);
    o.session = Some("conv-1".into());
    o.model = Some("m".into());
    o.effort = Some("low".into());
    o.reply = "Fine.".into();
    let mut run = opinion("agy");
    run.finish(Answer::parse(&to_json(&o)).as_ref(), "", false);
    assert_eq!(run.status, Status::Done);
    assert_eq!(run.delegated_session.as_deref(), Some("conv-1"));
    assert_eq!(run.model.as_deref(), Some("m"));
    assert_eq!(run.error, None);

    let mut failed = opinion("codex");
    failed.finish(None, "crash\n", false);
    assert_eq!(failed.status, Status::Failed);
    assert_eq!(failed.error.as_deref(), Some("crash"));
    let mut silent = opinion("codex");
    silent.finish(None, "  ", false);
    assert_eq!(
        silent.error.as_deref(),
        Some("The tool ended without an answer.")
    );
    let mut noisy = opinion("codex");
    noisy.finish(None, &"y".repeat(1000), false);
    assert_eq!(noisy.error.as_deref().map(str::len), Some(300));

    let mut empty = opinion("codex");
    o.reply = String::new();
    empty.finish(Answer::parse(&to_json(&o)).as_ref(), "", false);
    assert_eq!(empty.status, Status::Failed);
    assert_eq!(empty.error.as_deref(), Some("It sent no reply."));

    let mut cancelled = opinion("codex");
    cancelled.finish(None, "", true);
    assert_eq!(cancelled.status, Status::Cancelled);
    o.reply = "partial".into();
    let mut keeps = opinion("codex");
    keeps.finish(Answer::parse(&to_json(&o)).as_ref(), "", true);
    assert_eq!(keeps.status, Status::Cancelled);
    assert_eq!(keeps.reply, "partial");
    assert_eq!(keeps.delegated_session.as_deref(), Some("conv-1"));
}

#[test]
fn it_round_trips_through_json_with_camel_case_keys() {
    let mut run = opinion("agy");
    run.delegated_session = Some("c".into());
    let text = serde_json::to_string(&run).unwrap();
    assert!(text.contains("\"delegatedSession\":\"c\"") && text.contains("\"status\":\"running\""));
    assert_eq!(serde_json::from_str::<SecondOpinion>(&text).unwrap(), run);
    assert_eq!(run.tool_value(), Tool::Agy);
    assert_eq!(opinion("nonsense").tool_value(), Tool::Codex);
}

#[test]
fn model_options() {
    let o = agy_options("gemini-4-argon-low\tA\ngemini-4-argon-high\tA\nflash\tF\n");
    assert_eq!(
        o.iter().map(|m| m.model.as_str()).collect::<Vec<_>>(),
        ["gemini-4-argon", "flash"]
    );
    assert_eq!(o[0].efforts, ["low", "high"]);
    assert!(o[1].efforts.is_empty());
    let codex = crate::tools::codex::models::parse(
        br#"{"models":[{"slug":"x","display_name":"X","supported_reasoning_levels":[{"effort":"low"}]}]}"#,
    );
    let o = codex_options(&codex);
    assert_eq!((o[0].model.as_str(), o[0].name.as_str()), ("x", "X"));
    assert_eq!(o[0].efforts, ["low"]);
}
