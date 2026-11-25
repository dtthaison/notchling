mod common;

use common::*;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::*;
use notchling_engine::transcript::shells::{self, ShellState};
use notchling_engine::transcript::subagents::{self, Status, Subagent};
use notchling_engine::transcript::util::now_secs;
use std::collections::HashMap;

fn setup() -> (Scratch, String) {
    let scratch = Scratch::new("subagents");
    let agent = |id: &str, meta: &str, lines: &[&str]| {
        scratch.write(&format!("sess/subagents/agent-{id}.meta.json"), meta);
        scratch.write(
            &format!("sess/subagents/agent-{id}.jsonl"),
            &(lines.join("\n") + "\n"),
        );
    };
    agent(
        "abg",
        r#"{"agentType":"Explore","description":"Survey the repo","toolUseId":"toolu_bg","requestShape":"background","model":"sonnet","effort":"high"}"#,
        &[
            r#"{"isSidechain":true,"agentId":"abg","type":"user","message":{"role":"user","content":"Read-only survey of the repo."},"uuid":"u1","timestamp":"2026-11-05T10:00:00.000Z"}"#,
            r#"{"isSidechain":true,"agentId":"abg","type":"assistant","message":{"id":"m1","role":"assistant","model":"claude-sonnet-5-5","content":[{"type":"text","text":"The repo has three apps."}]},"uuid":"u2","timestamp":"2026-11-05T10:01:00.000Z"}"#,
        ],
    );
    agent(
        "afg",
        r#"{"agentType":"general-purpose","description":"Fix the test","toolUseId":"toolu_fg"}"#,
        &[],
    );
    let transcript = s(&scratch.write("sess.jsonl", ""));
    (scratch, transcript)
}

fn finished(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn lists_with_meta_and_status() {
    let (_scratch, transcript) = setup();
    let now = now_secs();
    let subs = subagents::list(&transcript, &finished(&[("abg", "completed")]), now);
    let mut ids: Vec<&str> = subs.iter().map(|a| a.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["claude:abg", "claude:afg"]);
    let bg = subs.iter().find(|a| a.id == "claude:abg").expect("abg");
    assert_eq!(bg.description, "Survey the repo");
    assert_eq!(bg.agent_type, "Explore");
    assert_eq!(bg.model.as_deref(), Some("sonnet"));
    assert!(bg.background);
    assert_eq!(bg.status, Status::Completed);
    let fg = subs.iter().find(|a| a.id == "claude:afg").expect("afg");
    assert_eq!(fg.status, Status::Running, "just written, no word it ended");
    let later = subagents::list(&transcript, &HashMap::new(), now + 3600.0);
    assert_eq!(
        later
            .iter()
            .find(|a| a.id == "claude:afg")
            .map(|a| a.status),
        Some(Status::Quiet)
    );
}

fn bare(background: bool) -> Subagent {
    Subagent {
        id: "abg".into(),
        path: String::new(),
        description: String::new(),
        agent_type: String::new(),
        model: None,
        effort: None,
        background,
        tool_use_id: Some("toolu_bg".into()),
        started: 0.0,
        updated: 0.0,
        status: Status::Quiet,
        origin: Default::default(),
        session: String::new(),
        parent: String::new(),
        depth: 1,
    }
}

#[test]
fn background_launch_result_does_not_end_it() {
    let now = 1.0;
    assert_eq!(
        subagents::status_of(&bare(true), &finished(&[("toolu_bg", "completed")]), now),
        Status::Running
    );
    assert_eq!(
        subagents::status_of(&bare(false), &finished(&[("toolu_bg", "failed")]), now),
        Status::Failed
    );
    assert_eq!(
        subagents::status_of(&bare(true), &finished(&[("abg", "killed")]), now),
        Status::Stopped
    );
}

#[test]
fn reads_notifications_from_the_session_transcript() {
    let (scratch, transcript) = setup();
    let note = "<task-notification>\\n<task-id>abg</task-id>\\n<status>completed</status>\\n<summary>done</summary>\\n</task-notification>";
    let lines = [
        format!(r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T10:02:00.000Z","content":"{note}"}}"#),
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_fg","content":"fixed"}]},"toolUseResult":{"status":"completed","agentId":"afg"},"timestamp":"2026-11-05T10:03:00.000Z"}"#.to_string(),
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_bg","content":"launched"}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"abg"},"timestamp":"2026-11-05T09:59:00.000Z"}"#.to_string(),
    ];
    scratch.write("sess.jsonl", &(lines.join("\n") + "\n"));
    let done = conversation::read(&transcript).finished_agents;
    assert_eq!(done.get("abg").map(String::as_str), Some("completed"));
    assert_eq!(done.get("afg").map(String::as_str), Some("completed"));
    assert_eq!(done.get("toolu_fg").map(String::as_str), Some("completed"));
    assert_eq!(
        done.get("toolu_bg"),
        None,
        "a background launch isn't an end"
    );
}

#[test]
fn reads_the_subagents_own_sidechain_transcript() {
    let (_scratch, transcript) = setup();
    let subs = subagents::list(&transcript, &HashMap::new(), now_secs());
    let sub = subs.iter().find(|a| a.id == "claude:abg").expect("abg");
    let c = subagents::read(&sub.path);
    let texts: Vec<String> = c
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::User { text, .. } | ItemKind::Assistant { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        ["Read-only survey of the repo.", "The repo has three apps."]
    );
    assert!(
        conversation::read(&sub.path).items.is_empty(),
        "a session's reader still skips sidechain lines"
    );
}

#[test]
fn active_count_for_the_notch() {
    let (_scratch, transcript) = setup();
    assert_eq!(subagents::active_count(&transcript, now_secs()), 2);
    assert_eq!(subagents::active_count(&transcript, now_secs() + 600.0), 0);
}

#[test]
fn a_handed_back_report_ends_a_quiet_agent() {
    let (scratch, transcript) = setup();
    scratch.write(
        "sess/subagents/agent-afg.jsonl",
        "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"content\":\"Report delivered to your caller\"}]}}\n",
    );
    let subs = subagents::list(&transcript, &HashMap::new(), now_secs());
    assert_eq!(
        subs.iter().find(|a| a.id == "claude:afg").map(|a| a.status),
        Some(Status::Completed)
    );
}

#[test]
fn notification_statuses_pair_ids_and_statuses() {
    let text = "<task-notification>\n<task-id> b1 </task-id>\n<status>killed</status>\n</task-notification> x <task-notification><task-id>b2</task-id></task-notification>";
    assert_eq!(
        subagents::notification_statuses(text),
        [("b1".to_string(), "killed".to_string())]
    );
    assert!(subagents::notification_statuses("nothing").is_empty());
}

#[test]
fn console_reader_lists_running_first() {
    let (scratch, transcript) = setup();
    scratch.write(
        "sess/subagents/agent-old.meta.json",
        r#"{"agentType":"Plan","description":"Old","toolUseId":"t","spawnDepth":2,"model":"opus","finished":true}"#,
    );
    scratch.write("sess/subagents/agent-old.jsonl", "\n");
    let list = subagents::console_list("sess", Some(&transcript), now_secs(), 600.0);
    assert_eq!(list.len(), 3);
    assert!(list[0].running && list[1].running);
    let old = list.iter().find(|a| a.id == "old").expect("old");
    assert!(!old.running);
    assert_eq!(old.depth, 2);
    assert_eq!(old.model, "opus");
    assert!(old.finished);
    assert!(subagents::console_list("sess", None, now_secs(), 600.0).is_empty());
}

#[test]
fn launch_reads_id_and_path() {
    let text = "Command running in background with ID: bj5rd6y90. Output is being written to: /private/tmp/claude-501/-Users-x/38b0/tasks/bj5rd6y90.output. You will be notified when it completes.";
    let launch = shells::launch(text).expect("launch");
    assert_eq!(launch.id, "bj5rd6y90");
    assert_eq!(
        launch.path,
        "/private/tmp/claude-501/-Users-x/38b0/tasks/bj5rd6y90.output"
    );
}

#[test]
fn launch_ignores_ordinary_output() {
    assert!(shells::launch("hello\nworld").is_none());
    assert!(shells::launch("Command running in background with ID: abc.").is_none());
}

#[test]
fn writers_skip_readers_and_claude() {
    let text = "p4010\nczsh\nf1\naw\nf2\naw\np4011\ncsleep\nf1\naw\np500\ncclaude\nf9\naw\np600\nctail\nf3\nar\n";
    assert_eq!(shells::parse_writers(text), [4010, 4011]);
}

#[test]
fn state_uses_notification_first() {
    let shell = BackgroundShell {
        id: "b1".into(),
        command: "sleep 9".into(),
        title: "t".into(),
        output_path: "/nonexistent/b1.output".into(),
        started: None,
    };
    let none = |_: &str| Vec::new();
    assert_eq!(
        shells::state(&shell, &finished(&[("b1", "killed")]), &none),
        ShellState::Ended("killed".into())
    );
    assert_eq!(
        shells::state(&shell, &HashMap::new(), &none),
        ShellState::Ended("ended".into())
    );
    assert_eq!(
        shells::state(&shell, &HashMap::new(), &|_: &str| vec![1]),
        ShellState::Running
    );
}
