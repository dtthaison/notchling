//! Port of `VizEngineTests`, `WebURLCandidateTests` and `VisualizeSuggesterTests`.

use super::engine::{GitSnapshot, VizEngine};
use super::facts::{analyze_lines, mermaid_sources, TranscriptFacts};
use crate::bridge::model::{Control, Session, SessionSubagent};
use crate::sessions::testutil::TempDir;
use notchling_protocol::generated as wire;
use std::path::Path;

/// 2026-11-01 13:57:45 UTC, in seconds: the Swift tests' `fixtureNow`.
const NOW: f64 = 1_793_541_465.0;

fn assistant(blocks: &str, id: &str, time: &str, usage: Option<&str>) -> String {
    let usage = usage.map_or(String::new(), |u| format!(r#","usage":{u}"#));
    format!(
        r#"{{"type":"assistant","uuid":"u-{id}","timestamp":"{time}","message":{{"id":"{id}","role":"assistant"{usage},"content":[{blocks}]}}}}"#
    )
}

fn user(text: &str, time: &str, sidechain: bool) -> String {
    format!(
        r#"{{"type":"user","uuid":"u-{text}","isSidechain":{sidechain},"timestamp":"{time}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn tool(name: &str, input: &str) -> String {
    format!(r#"{{"type":"tool_use","id":"t","name":"{name}","input":{input}}}"#)
}

fn analyze(lines: &[String], is_main: bool, origin: &str) -> TranscriptFacts {
    analyze_lines(lines, is_main, origin, "")
}

fn no_git() -> GitSnapshot {
    GitSnapshot::default()
}

fn engine() -> VizEngine {
    VizEngine::new(Box::new(|| NOW), Box::new(|_| no_git()))
}

#[test]
fn flow_groups_tool_calls_into_turns_and_collects_files() {
    let long =
        "flutter analyze lib/features/login --no-pub && echo this part is cut off past sixty chars";
    let lines = vec![
        user("fix the login bug", "2026-11-01T10:00:00.000Z", false),
        assistant(
            &tool("Read", r#"{"file_path":"/p/lib/login.dart"}"#),
            "a1",
            "2026-11-01T10:00:01.000Z",
            None,
        ),
        assistant(
            &format!(
                "{},{}",
                tool("Edit", r#"{"file_path":"/p/lib/login.dart"}"#),
                tool("Bash", &format!(r#"{{"command":"{long}"}}"#))
            ),
            "a2",
            "2026-11-01T10:00:02.000Z",
            None,
        ),
        assistant(
            &tool(
                "Agent",
                r#"{"description":"Research the API","subagent_type":"general-purpose"}"#,
            ),
            "a3",
            "2026-11-01T10:00:03.000Z",
            None,
        ),
        user("now add a test", "2026-11-01T10:05:00.000Z", false),
        assistant(
            &tool("Write", r#"{"file_path":"/p/test/login_test.dart"}"#),
            "a4",
            "2026-11-01T10:05:01.000Z",
            None,
        ),
        user("sidechain noise", "2026-11-01T10:06:00.000Z", true),
    ];
    let facts = analyze(&lines, true, "main");
    assert_eq!(
        facts.turns.len(),
        2,
        "the sidechain user line does not open a turn"
    );
    assert_eq!(facts.turns[0].prompt, "fix the login bug");
    let tools: Vec<&str> = facts.turns[0]
        .calls
        .iter()
        .map(|c| c.tool.as_str())
        .collect();
    assert_eq!(tools, ["Read", "Edit", "Bash", "Agent"]);
    let targets: Vec<&str> = facts.turns[0]
        .calls
        .iter()
        .map(|c| c.target.as_str())
        .collect();
    let cut: String = long.chars().take(60).collect();
    assert_eq!(
        targets,
        ["login.dart", "login.dart", cut.as_str(), "Research the API"]
    );
    assert_eq!(facts.turns[0].calls[2].target.chars().count(), 60);
    let subagent: Vec<bool> = facts.turns[0].calls.iter().map(|c| c.subagent).collect();
    assert_eq!(subagent, [false, false, false, true]);
    let edit: Vec<bool> = facts.turns[0].calls.iter().map(|c| c.edit).collect();
    assert_eq!(edit, [false, true, false, false]);
    assert_eq!(facts.turns[1].calls.len(), 1);
    assert_eq!(facts.turns[1].calls[0].tool, "Write");
    assert_eq!(facts.file_edits.len(), 2, "Read is not an edit");
    assert_eq!(facts.file_edits["/p/lib/login.dart"], 1);
    assert_eq!(facts.file_edits["/p/test/login_test.dart"], 1);
    assert_eq!(facts.tool_counts["Read"], 1);
}

#[test]
fn tool_calls_before_any_prompt_land_in_an_implicit_turn() {
    let lines = vec![assistant(
        &tool("Read", r#"{"file_path":"/p/a.swift"}"#),
        "x",
        "2026-11-01T10:00:01.000Z",
        None,
    )];
    let facts = analyze(&lines, true, "main");
    assert_eq!(facts.turns.len(), 1);
    assert_eq!(facts.turns[0].prompt, "");
}

#[test]
fn flow_spec_shows_files_branch_and_diff_stat() {
    let edits = format!(
        "{},{},{}",
        tool("Edit", r#"{"file_path":"/p/a.swift"}"#),
        tool("Edit", r#"{"file_path":"/p/a.swift"}"#),
        tool("Write", r#"{"file_path":"/p/b.swift"}"#)
    );
    let lines = vec![
        user("go", "2026-11-01T10:00:00.000Z", false),
        assistant(&edits, "a", "2026-11-01T10:00:01.000Z", None),
    ];
    let facts = analyze(&lines, true, "main");
    let git = VizEngine::new(
        Box::new(|| NOW),
        Box::new(|_| GitSnapshot {
            branch: Some("feat/x".into()),
            diff_stat: Some(" a.swift | 4 ++--\n 1 file changed".into()),
        }),
    );
    let spec = git.flow_spec(&facts, &facts, "/p");
    let flow = spec.flow.expect("flow");
    assert_eq!(flow.branch.as_deref(), Some("feat/x"));
    assert!(flow.diff_stat.expect("stat").contains("1 file changed"));
    let names: Vec<&str> = flow.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["a.swift", "b.swift"]);
    let counts: Vec<i64> = flow.files.iter().map(|f| f.count).collect();
    assert_eq!(counts, [2, 1]);
    assert_eq!(flow.total_calls, 3);

    let empty = engine().flow_spec(&TranscriptFacts::default(), &TranscriptFacts::default(), "");
    assert!(
        empty.empty.is_some(),
        "nothing ran yet: empty state, not a blank flow"
    );
    assert!(empty.flow.is_none());
}

#[test]
fn flow_keeps_the_newest_forty_turns_and_three_hundred_calls() {
    let mut lines = Vec::new();
    for turn in 0..50 {
        lines.push(user(
            &format!("prompt {turn}"),
            "2026-11-01T10:00:00.000Z",
            false,
        ));
        lines.push(assistant(
            &vec![tool("Read", r#"{"file_path":"/p/a"}"#); 10].join(","),
            &format!("m{turn}"),
            "2026-11-01T10:00:01.000Z",
            None,
        ));
    }
    let facts = analyze(&lines, true, "main");
    let flow = engine().flow_spec(&facts, &facts, "/p").flow.expect("flow");
    assert_eq!(flow.total_calls, 500);
    let shown: usize = flow.turns.iter().map(|t| t.calls.len()).sum();
    assert_eq!(shown, 300, "the budget stops at the newest 300 calls");
    assert_eq!(
        flow.turns.last().map(|t| t.prompt.as_str()),
        Some("prompt 49")
    );
    assert_eq!(flow.turns.len(), 30);
}

#[test]
fn mermaid_blocks_are_extracted_newest_first_from_main_and_subagents() {
    let text = "Here:\\n```mermaid\\ngraph TD\\n  A-->B\\n```\\nand\\n```swift\\nlet x = 1\\n```\\n```mermaid\\nsequenceDiagram\\n  A->>B: hi\\n```";
    let main = analyze(
        &[assistant(
            &format!(r#"{{"type":"text","text":"{text}"}}"#),
            "m1",
            "2026-11-01T10:00:00.000Z",
            None,
        )],
        true,
        "main",
    );
    let sources: Vec<&str> = main.mermaid.iter().map(|m| m.source.as_str()).collect();
    assert_eq!(
        sources,
        ["graph TD\n  A-->B", "sequenceDiagram\n  A->>B: hi"]
    );
    assert!(mermaid_sources("no fences").is_empty());
    assert!(
        mermaid_sources("```mermaid\n```").is_empty(),
        "empty block ignored"
    );

    let sub = analyze(
        &[assistant(
            r#"{"type":"text","text":"```mermaid\nflowchart LR\n  X-->Y\n```"}"#,
            "s1",
            "2026-11-01T11:00:00.000Z",
            None,
        )],
        false,
        "Scan the repo",
    );
    let mut combined = main;
    combined.merge(sub);
    let spec = VizEngine::diagram_spec(&combined);
    let blocks = spec.diagram.expect("diagram").blocks;
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].origin, "Scan the repo", "newest first");
    assert_eq!(blocks[0].source, "flowchart LR\n  X-->Y");
    assert_eq!(blocks[0].label, "1. flowchart LR (Scan the repo)");
    assert!(VizEngine::diagram_spec(&TranscriptFacts::default())
        .empty
        .is_some());
}

#[test]
fn usage_is_extracted_and_deduped_by_message_id() {
    let usage1 = r#"{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000}"#;
    let usage2 = r#"{"input_tokens":10,"output_tokens":50,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000}"#;
    let usage3 = r#"{"input_tokens":7,"output_tokens":3}"#;
    let text = r#"{"type":"text","text":"hi"}"#;
    let facts = analyze(
        &[
            assistant(text, "msg1", "2026-11-01T10:00:00.000Z", Some(usage1)),
            assistant(text, "msg1", "2026-11-01T10:00:00.500Z", Some(usage2)),
            assistant(text, "msg2", "2026-11-01T10:01:00.000Z", Some(usage3)),
        ],
        true,
        "main",
    );
    assert_eq!(
        facts.usage.len(),
        2,
        "a streamed message counts once, with its last usage"
    );
    assert_eq!(facts.usage[0].output, 50);
    assert_eq!(facts.usage[0].cache_read, 1000);
    assert_eq!(
        (
            facts.usage[1].input,
            facts.usage[1].output,
            facts.usage[1].cache_creation
        ),
        (7, 3, 0)
    );
    let chart = VizEngine::chart_spec(&facts, &[]).chart.expect("chart");
    assert_eq!(
        (
            chart.totals.input,
            chart.totals.output,
            chart.totals.cache_creation,
            chart.totals.cache_read
        ),
        (17, 53, 100, 1000)
    );
    assert_eq!(chart.tokens.len(), 2);
}

fn subagent(id: &str, model: &str) -> wire::Subagent {
    wire::Subagent {
        id: id.into(),
        r#type: "t".into(),
        description: "d".into(),
        model: model.into(),
        depth: 1,
        finished: true,
        state: wire::SubagentState::Done,
        last_activity: NOW * 1000.0,
    }
}

#[test]
fn chart_counts_tools_and_subagent_models() {
    let lines = vec![assistant(
        &format!(
            "{},{},{}",
            tool("Read", "{}"),
            tool("Read", "{}"),
            tool("Bash", r#"{"command":"ls"}"#)
        ),
        "a",
        "2026-11-01T10:00:00.000Z",
        None,
    )];
    let facts = analyze(&lines, true, "main");
    let agents = [
        subagent("1", "haiku"),
        subagent("2", "sonnet"),
        subagent("3", "haiku"),
    ];
    let chart = VizEngine::chart_spec(&facts, &agents).chart.expect("chart");
    let tools: Vec<(&str, i64)> = chart
        .tools
        .iter()
        .map(|t| (t.name.as_str(), t.count))
        .collect();
    assert_eq!(tools, [("Read", 2), ("Bash", 1)]);
    let models: Vec<(&str, i64)> = chart
        .models
        .iter()
        .map(|m| (m.model.as_str(), m.count))
        .collect();
    assert_eq!(models, [("haiku", 2), ("sonnet", 1)]);
    assert!(VizEngine::chart_spec(&TranscriptFacts::default(), &[])
        .empty
        .is_some());
}

#[test]
fn chart_thins_a_long_history_to_three_hundred_points() {
    let text = r#"{"type":"text","text":"hi"}"#;
    let usage = r#"{"input_tokens":1,"output_tokens":1}"#;
    let lines: Vec<String> = (0..1000)
        .map(|n| {
            let time = format!("2026-11-01T10:{:02}:{:02}.000Z", n / 60, n % 60);
            assistant(text, &format!("m{n}"), &time, Some(usage))
        })
        .collect();
    let facts = analyze(&lines, true, "main");
    let chart = VizEngine::chart_spec(&facts, &[]).chart.expect("chart");
    assert_eq!(chart.tokens.len(), 300);
    assert_eq!(
        chart.totals.input, 1000,
        "totals count every point, not the thinned ones"
    );
}

#[test]
fn localhost_urls_and_device_hints_are_collected() {
    let lines = vec![
        assistant(
            &tool("Bash", r#"{"command":"xcrun simctl boot 'iPhone 16 Pro' && xcrun simctl list"}"#),
            "d",
            "2026-11-01T10:00:00.000Z",
            None,
        ),
        r#"{"type":"user","uuid":"r","message":{"role":"user","content":[{"type":"tool_result","content":"ready on http://localhost:3000/app, also https://example.com/x and http://127.0.0.1:8080."}]}}"#.to_string(),
    ];
    let facts = analyze(&lines, true, "main");
    assert_eq!(
        facts.local_urls,
        ["http://localhost:3000/app", "http://127.0.0.1:8080"]
    );
    assert_eq!(facts.device_hints, ["iPhone 16 Pro"]);
}

#[test]
fn only_conversation_text_feeds_the_urls() {
    let dir = TempDir::new("viz-urls");
    let lines = [
        r#"{"type":"queue-operation","operation":"enqueue","content":"see http://localhost:9999/queued"}"#,
        r#"{"type":"attachment","attachment":{"type":"x","text":"http://localhost:9998/attachment"}}"#,
        r#"{"type":"assistant","message":{"id":"a","content":[{"type":"tool_use","name":"Bash","input":{"command":"curl http://localhost:9997/api"}},{"type":"text","text":"Open http://localhost:5173/ now"}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"Local: http://localhost:4321/"}]}}"#,
    ];
    let file = dir.write("t.jsonl", &lines.join("\n"));
    let facts = super::facts::analyze_file(&file.to_string_lossy(), true, "main", 1_000_000);
    assert_eq!(
        facts.local_urls,
        ["http://localhost:5173/", "http://localhost:4321/"]
    );
}

// MARK: The engine on a real transcript

fn fixture_session(transcript: &Path) -> Session {
    let mut session = Session::new("fx", "n", "", Control::ViewOnly, wire::SessionState::Done);
    session.transcript_path = Some(transcript.to_string_lossy().into_owned());
    session
}

#[test]
fn the_recent_mermaid_signal_uses_the_clock() {
    let dir = TempDir::new("viz-clock");
    let line = assistant(
        r#"{"type":"text","text":"```mermaid\ngraph TD\n A-->B\n```"}"#,
        "m",
        "2026-11-01T13:50:00.000Z",
        None,
    );
    let file = dir.write("t.jsonl", &format!("{line}\n"));
    let mut session = fixture_session(&file);
    // NOW is 13:57:45, seven minutes after the block.
    assert!(engine().build(&session, None).signals.has_recent_mermaid);
    let later = VizEngine::new(Box::new(|| NOW + 3600.0), Box::new(|_| no_git()));
    assert!(!later.build(&session, None).signals.has_recent_mermaid);
    session.transcript_path = None;
    assert!(!engine().build(&session, None).signals.has_recent_mermaid);
}

#[test]
fn a_subagents_transcript_adds_to_the_session_and_changes_invalidate_the_cache() {
    let dir = TempDir::new("viz-sub");
    let main = dir.write(
        "t.jsonl",
        &(user("go", "2026-11-01T10:00:00.000Z", false) + "\n"),
    );
    let sub = dir.write(
        "sub.jsonl",
        &(assistant(
            r#"{"type":"text","text":"```mermaid\nflowchart LR\n  X-->Y\n```"}"#,
            "s1",
            "2026-11-01T13:55:00.000Z",
            None,
        ) + "\n"),
    );
    let mut session = fixture_session(&main);
    let mut info = subagent("1", "haiku");
    info.description = "Scan the repo".into();
    session.subagents.push(SessionSubagent {
        info,
        transcript_path: Some(sub.to_string_lossy().into_owned()),
    });
    let viz = engine();
    let first = viz.build(&session, None);
    let blocks = first.diagram.diagram.expect("diagram").blocks;
    assert_eq!(blocks[0].origin, "Scan the repo");
    assert!(first.signals.has_recent_mermaid);

    // The file grows: the cached facts are replaced, not served again.
    std::fs::write(
        &sub,
        format!(
            "{}\n{}\n",
            assistant(
                r#"{"type":"text","text":"```mermaid\nflowchart LR\n  X-->Y\n```"}"#,
                "s1",
                "2026-11-01T13:55:00.000Z",
                None
            ),
            assistant(
                r#"{"type":"text","text":"```mermaid\nsequenceDiagram\n  A->>B: hi\n```"}"#,
                "s2",
                "2026-11-01T13:56:00.000Z",
                None
            )
        ),
    )
    .expect("grow");
    let second = viz.build(&session, None);
    assert_eq!(second.diagram.diagram.expect("diagram").blocks.len(), 2);
}

#[test]
fn entity_asks_the_schema_reader_and_says_so_when_there_is_none() {
    let viz = engine();
    let none = viz.entity_spec("");
    assert_eq!(none.kind, wire::VizKind::Entity);
    assert_eq!(none.empty.expect("empty").title, "Session chưa có thư mục");
    let unsupported = viz.entity_spec("/p");
    assert!(unsupported.entity.is_none());
    assert!(unsupported
        .empty
        .expect("empty")
        .detail
        .expect("detail")
        .contains("chưa"));
}

#[test]
fn spec_json_is_stable() {
    let spec = wire::VizSpec {
        kind: wire::VizKind::Flow,
        empty: None,
        error: None,
        flow: Some(wire::FlowSpec {
            branch: Some("main".into()),
            diff_stat: None,
            files: vec![wire::FlowFile {
                path: "/a/b.swift".into(),
                name: "b.swift".into(),
                count: 2,
            }],
            turns: vec![wire::FlowTurn {
                index: 1,
                prompt: "go".into(),
                calls: vec![wire::FlowCall {
                    tool: "Edit".into(),
                    target: "b.swift".into(),
                    subagent: false,
                    edit: true,
                    time: Some(1000.0),
                }],
            }],
            total_calls: 1,
        }),
        diagram: None,
        entity: None,
        chart: None,
    };
    let expected = r#"{"flow":{"branch":"main","files":[{"count":2,"name":"b.swift","path":"/a/b.swift"}],"totalCalls":1,"turns":[{"calls":[{"edit":true,"subagent":false,"target":"b.swift","time":1000,"tool":"Edit"}],"index":1,"prompt":"go"}]},"kind":"flow"}"#;
    assert_eq!(
        crate::bridge::wire::text(&crate::bridge::wire::object(&spec)),
        expected
    );
    let empty = wire::VizSpec {
        kind: wire::VizKind::Chart,
        empty: Some(wire::VizMessage {
            title: "T".into(),
            detail: None,
        }),
        error: None,
        flow: None,
        diagram: None,
        entity: None,
        chart: None,
    };
    assert_eq!(
        crate::bridge::wire::text(&crate::bridge::wire::object(&empty)),
        r#"{"empty":{"title":"T"},"kind":"chart"}"#
    );
}
