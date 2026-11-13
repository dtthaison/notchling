mod common;

use common::*;
use notchling_engine::transcript::codex;
use notchling_engine::transcript::model::*;

const LINES: [&str; 14] = [
    r#"{"timestamp":"2026-11-05T08:44:11.961Z","ordinal":0,"type":"session_meta","payload":{"id":"01a1aae1-432d","cwd":"/tmp/proj","originator":"codex_exec"}}"#,
    r#"{"timestamp":"2026-11-05T08:44:12.000Z","ordinal":1,"type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#,
    r#"{"timestamp":"2026-11-05T08:44:12.001Z","ordinal":2,"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"You are Codex"}]}}"#,
    r#"{"timestamp":"2026-11-05T08:44:12.002Z","ordinal":3,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>\n<cwd>/tmp/proj</cwd>\n</environment_context>"}]}}"#,
    r#"{"timestamp":"2026-11-05T08:44:12.003Z","ordinal":4,"type":"turn_context","payload":{"turn_id":"t1","model":"gpt-6-luna"}}"#,
    r#"{"timestamp":"2026-11-05T08:44:12.004Z","ordinal":5,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Fix the failing test\nin parser.swift"}]}}"#,
    r#"{"timestamp":"2026-11-05T08:44:13.000Z","ordinal":6,"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Looking at the test first."}]}}"#,
    r#"{"timestamp":"2026-11-05T08:44:14.000Z","ordinal":7,"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"c1","arguments":"{\"cmd\":\"swift test\"}"}}"#,
    r#"{"timestamp":"2026-11-05T08:44:15.000Z","ordinal":8,"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"Process exited with code 1\nOutput:\n1 failure"}}"#,
    r#"{"timestamp":"2026-11-05T08:44:16.000Z","ordinal":9,"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"c2","input":"*** Begin Patch\n*** Update File: Sources/parser.swift\n@@\n-let a = 1\n+let a = 2\n*** End Patch"}}"#,
    r#"{"timestamp":"2026-11-05T08:44:17.000Z","ordinal":10,"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c2","output":"Success. Updated the following files:\nM Sources/parser.swift"}}"#,
    r#"{"timestamp":"2026-11-05T08:44:18.000Z","ordinal":11,"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Fixed it."}]}}"#,
    r#"{"timestamp":"2026-11-05T08:44:18.100Z","ordinal":12,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"output_tokens":50},"last_token_usage":{"input_tokens":900,"output_tokens":40}}}}"#,
    r#"{"timestamp":"2026-11-05T08:44:18.200Z","ordinal":13,"type":"event_msg","payload":{"type":"task_complete","turn_id":"t1","duration_ms":6200}}"#,
];

fn write(scratch: &Scratch, lines: &[&str]) -> String {
    s(&scratch.write(
        "rollout-2026-11-05T15-44-11-01a1aae1-432d.jsonl",
        &(lines.join("\n") + "\n"),
    ))
}

#[test]
fn meta_and_title_skip_injected_messages() {
    let scratch = Scratch::new("codex");
    let path = write(&scratch, &LINES);
    let meta = codex::meta(&path).expect("meta");
    assert_eq!(meta.id, "01a1aae1-432d");
    assert_eq!(meta.cwd, "/tmp/proj");
    assert_eq!(
        meta.title.as_deref(),
        Some("Fix the failing test"),
        "first line of what you typed, not the environment context"
    );
}

#[test]
fn conversation() {
    let scratch = Scratch::new("codex");
    let path = write(&scratch, &LINES);
    let c = codex::read(&path);
    let kinds: Vec<String> = c
        .items
        .iter()
        .map(|item| match &item.kind {
            ItemKind::User { text, .. } => format!("user:{text}"),
            ItemKind::Assistant { text } => format!("assistant:{text}"),
            ItemKind::Thinking { text, .. } => format!("thinking:{text}"),
            ItemKind::Tools(calls) => {
                format!(
                    "tools:{}",
                    calls
                        .iter()
                        .map(|c| c.title.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
            ItemKind::TurnEnd { seconds, model } => {
                format!("end:{seconds}:{}", model.clone().unwrap_or_default())
            }
            _ => "other".into(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "user:Fix the failing test\nin parser.swift",
            "thinking:Looking at the test first.",
            "tools:swift test,Edit parser.swift",
            "assistant:Fixed it.",
            "end:6.2:gpt-6-luna"
        ]
    );
    let ItemKind::Tools(calls) = &c.items[2].kind else {
        panic!("tools")
    };
    assert_eq!(calls[0].kind, ToolKind::Shell);
    assert!(calls[0].is_error, "exit code 1");
    assert_eq!(calls[1].kind, ToolKind::Edit);
    assert_eq!(calls[1].added(), 1);
    assert_eq!(calls[1].removed(), 1);
    assert!(calls.iter().all(|c| c.finished));
    assert_eq!(c.facts.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(c.facts.context_tokens, 940);
    assert_eq!(c.facts.files, ["Sources/parser.swift"]);
    assert_eq!(c.turn_start, None, "the turn ended");
    assert!(!codex::turn_open(&path));
}

#[test]
fn an_open_turn_shows_what_its_doing() {
    let scratch = Scratch::new("codex");
    let path = write(&scratch, &LINES[..8]);
    let c = codex::read(&path);
    assert!(c.turn_start.is_some());
    assert_eq!(c.activity.as_deref(), Some("swift test"));
    assert!(codex::turn_open(&path));
}

#[test]
fn effort_from_the_turn_context() {
    let scratch = Scratch::new("codex");
    let mut lines = LINES.to_vec();
    lines[4] = r#"{"timestamp":"2026-11-05T08:44:12.003Z","ordinal":4,"type":"turn_context","payload":{"turn_id":"t1","model":"gpt-6-luna","collaboration_mode":{"mode":"default","settings":{"model":"gpt-6-luna","reasoning_effort":"xhigh"}}}}"#;
    let path = write(&scratch, &lines);
    assert_eq!(codex::read(&path).facts.effort.as_deref(), Some("xhigh"));
}

#[test]
fn the_recorded_rollout_reads_with_its_model_and_tokens() {
    let path = s(&notchling_fixture("codex-rollout.jsonl"));
    let meta = codex::meta(&path);
    assert_eq!(
        meta.as_ref().map(|m| m.cwd.as_str()),
        Some("/Users/me/work/demo")
    );
    let c = codex::read(&path);
    assert_eq!(c.facts.model.as_deref(), Some("gpt-5.1"));
    assert!(c.facts.input_tokens >= 45000);
    assert!(c.facts.context_tokens > 0);
}

#[test]
fn patch_files_and_diff_shapes() {
    let patch = "*** Begin Patch\n*** Update File: a/b.swift\n*** Add File: c.txt\n*** Delete File: d.txt\n@@ ctx\n keep\n-old\n+new\n*** End Patch";
    assert_eq!(codex::patch_files(patch), ["a/b.swift", "c.txt", "d.txt"]);
    let kinds: Vec<DiffKind> = codex::diff(patch).iter().map(|d| d.kind).collect();
    assert_eq!(
        kinds,
        [
            DiffKind::Hunk,
            DiffKind::Context,
            DiffKind::Remove,
            DiffKind::Add
        ]
    );
}

#[test]
fn shell_call_shapes() {
    let call = codex::call("c", "shell", r#"{"command":["bash","-lc","ls -la"]}"#, None);
    assert_eq!(call.kind, ToolKind::Shell);
    assert_eq!(call.input, "ls -la");
    let call = codex::call("c", "web_search", r#"{"query":"rust"}"#, None);
    assert_eq!(call.kind, ToolKind::Web);
    assert_eq!(call.title, "rust");
    let call = codex::call("c", "mystery", "raw", None);
    assert_eq!(call.kind, ToolKind::Other);
    assert_eq!(call.title, "mystery");
}

#[test]
fn output_text_unwraps_exec_json() {
    use serde_json::json;
    assert_eq!(
        codex::output_text(&json!(r#"{"output":"hi","metadata":{}}"#)),
        "hi"
    );
    assert_eq!(codex::output_text(&json!("plain")), "plain");
    assert_eq!(
        codex::output_text(&json!([{"text":"a"},{"text":"b"}])),
        "a\nb"
    );
}
