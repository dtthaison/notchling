//! The read-only Chat's view models over the fixture data root (`fixtures/chat`, laid out as the
//! notch's: `claude`, `notchling`, `codex`), on every system: what the sidebar lists, what the
//! conversation shows, what the header and inspector say. Nothing here reads the real `~/.claude`.

use std::path::{Path, PathBuf};

use notchling_app::chat::data::{DataRoot, Tool};
use notchling_app::chat::header::header;
use notchling_app::chat::inspector::{inspector, no_writers};
use notchling_app::chat::rows::{rows, Row};
use notchling_app::chat::sidebar::sections;
use notchling_kit::{Icon, Status};

/// Late 2026-11-08, the fixture's day.
const NOW_MS: i64 = 1_794_182_400_000;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/chat")
}

#[test]
fn sidebar_lists_the_fixture_sessions_in_group_order() {
    let root = DataRoot::fixture(&fixture_root());
    let sessions = root.sessions(NOW_MS);
    let names: Vec<&str> = sessions.iter().map(|s| s.agent.name.as_str()).collect();
    assert_eq!(names.len(), 4, "{names:?}");
    assert_eq!(names[0], "fix the bug", "needs input first");
    assert_eq!(names[1], "port search", "then working");
    assert!(names[2..].contains(&"shell mode"));
    assert!(
        names[2..].contains(&"Make the demo build again."),
        "the Codex row"
    );
    assert!(sessions.iter().any(|s| s.tool == Tool::Codex), "{names:?}");

    let sections = sections(&sessions, NOW_MS);
    assert_eq!(
        sections.iter().map(|s| s.title).collect::<Vec<_>>(),
        ["Needs input", "Working", "Done"]
    );
    assert_eq!(
        sections[0].rows[0].status_line,
        "answer: Update the README now? (Now · After the tests)"
    );
    assert_eq!(sections[0].rows[0].status, Status::Needs);
    assert_eq!(sections[2].rows.len(), 2);
}

#[test]
fn transcripts_in_the_fixture_are_read_relative_to_the_root() {
    let root = DataRoot::fixture(&fixture_root());
    let sessions = root.sessions(NOW_MS);
    let fix = sessions
        .iter()
        .find(|s| s.agent.name == "fix the bug")
        .expect("fix the bug");
    assert!(
        fix.transcript.as_ref().is_some_and(|p| p.is_file()),
        "{:?}",
        fix.transcript
    );
    let c = root.conversation(fix);
    let rows = rows(&c);
    assert!(rows.len() >= 12, "{} rows", rows.len());
    assert_eq!(
        rows[0],
        Row::User {
            text: "Fix the failing search test and port the ranking to Rust.".into(),
            images: 0,
            queued: false
        }
    );
    assert!(matches!(&rows[1], Row::Thinking { title, .. } if title == "Thought for 4s"));
    let tools: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Tools(t) => Some(t),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(tools
        .iter()
        .any(|t| t.icon == Icon::Terminal && t.output.is_some()));
    assert!(
        tools.iter().any(|t| t.added == 2 && t.removed == 2),
        "the edit's diff"
    );
    assert!(
        tools.iter().any(|t| t.icon == Icon::Agent),
        "the Agent call"
    );
    assert!(rows
        .iter()
        .any(|r| matches!(r, Row::TurnEnd { text } if text == "Worked for 48s · Opus 5.5")));
    assert!(rows.iter().any(|r| matches!(
        r,
        Row::Notice {
            icon: Icon::Info,
            ..
        }
    )));
    assert!(rows
        .iter()
        .any(|r| matches!(r, Row::CommandOutput { text } if text.contains("$1.84"))));
    assert_eq!(
        rows.last(),
        Some(&Row::User {
            text: "And bump the version.".into(),
            images: 0,
            queued: true
        })
    );
}

#[test]
fn a_reads_numbered_output_lines_up_in_the_view_model() {
    let root = DataRoot::fixture(&fixture_root());
    let sessions = root.sessions(NOW_MS);
    let fix = sessions
        .iter()
        .find(|s| s.agent.name == "fix the bug")
        .expect("fix the bug");
    let rows = rows(&root.conversation(fix));
    let read = rows
        .iter()
        .filter_map(|r| match r {
            Row::Tools(t) => Some(t),
            _ => None,
        })
        .flatten()
        .find(|t| t.title.starts_with("Read "))
        .expect("the Read call");
    // Claude Code's `cat -n` output is `     1<tab>code`; the engine trims the first line's
    // leading spaces, so the first number must not drag its code left of the others.
    let out = read.output.as_ref().expect("its output");
    assert!(!out.text.contains('\t'), "{:?}", out.text);
    let lines: Vec<&str> = out.text.lines().collect();
    assert_eq!(lines[0], "1  import Foundation");
    assert_eq!(lines[1], "2");
    assert_eq!(lines[2], "3  enum SessionSearch {");
    assert_eq!(
        lines[3],
        "4      static func rank(_ hits: [Hit]) -> [Hit] {"
    );
    assert_eq!(lines[6], "7  }");
}

#[test]
fn header_and_inspector_read_the_facts() {
    let root = DataRoot::fixture(&fixture_root());
    let sessions = root.sessions(NOW_MS);
    let fix = sessions
        .iter()
        .find(|s| s.agent.name == "fix the bug")
        .expect("fix the bug");
    let c = root.conversation(fix);
    let h = header(fix, &c, &root.home);
    assert_eq!(h.title, "fix the bug");
    assert_eq!(h.status, Status::Needs);
    assert_eq!(h.status_word, "Needs input");
    assert_eq!(h.model.as_deref(), Some("Opus 5.5"));
    assert_eq!(h.context.as_deref(), Some("28.1k / 400k · 7%"));
    assert_eq!(h.shells, 1);

    let subagents = root.subagents(fix, &c, NOW_MS as f64 / 1000.0);
    assert_eq!(subagents.len(), 1);
    let i = inspector(&c, &subagents, &root.home, &no_writers);
    assert_eq!(i.tasks.len(), 3);
    assert_eq!(i.tasks[0].status, Status::Done);
    assert!(i.facts.contains(&("Branch", "m1-chat".to_owned())));
    assert!(i.facts.contains(&("Effort", "high".to_owned())));
    assert_eq!(i.subagents[0].description, "Second opinion on the ranking");
    assert_eq!(i.subagents[0].status, Status::Done);
    assert_eq!(i.shells[0].title, "Run the Rust search tests");
}

#[test]
fn a_codex_row_reads_its_rollout() {
    let root = DataRoot::fixture(&fixture_root());
    let sessions = root.sessions(NOW_MS);
    let codex = sessions
        .iter()
        .find(|s| s.tool == Tool::Codex)
        .expect("a Codex row");
    assert!(codex.agent.id.starts_with("codex:"));
    assert_eq!(codex.agent.name, "Make the demo build again.");
    let c = root.conversation(codex);
    let rows = rows(&c);
    assert!(rows.len() >= 4, "{} rows", rows.len());
    assert!(rows
        .iter()
        .any(|r| matches!(r, Row::Tools(t) if t.iter().any(|t| t.is_error))));
    let h = header(codex, &c, &root.home);
    assert_eq!(h.host, "Codex · Codex");
    assert_eq!(h.model.as_deref(), Some("Gpt 6"));
    assert_eq!(h.context.as_deref(), Some("1.9k"));
}
