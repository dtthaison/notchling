use crate::agent_named::AgentNames;
use crate::late_emit::LateSalt;
use notchling_engine::ask::evidence::{command_hash, output_hash};
use notchling_events::field::FieldValue;
use notchling_events::ids::new_op_id;
use notchling_events::kinds::choices::AgentTool;
use notchling_events::scope::Scope;
use notchling_events::{Event, MemorySink, Salt};
use notchling_local_api::{AgentCall, AgentIdentity};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const MAIN: &str = include_str!("../../engine/tests/fixtures/ask/main.jsonl");
const A1: &str = include_str!("../../engine/tests/fixtures/ask/subagents/agent-a1.jsonl");
const A1_META: &str = include_str!("../../engine/tests/fixtures/ask/subagents/agent-a1.meta.json");
const A2: &str = include_str!("../../engine/tests/fixtures/ask/subagents/agent-a2.jsonl");
const A2_META: &str = include_str!("../../engine/tests/fixtures/ask/subagents/agent-a2.meta.json");

const SESSION: &str = "11111111-2222-4333-8444-555555555555";
const TASKS: &str =
    "/private/tmp/claude-501/-Users-dev-work/11111111-2222-4333-8444-555555555555/tasks";
const SALT: [u8; 32] = [5; 32];
const AT_MS: i64 = 1_794_823_271_500;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(tag: &str) -> (Dir, PathBuf) {
    let dir = std::env::temp_dir().join(format!("nl-named-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join("projects/-Users-dev-work");
    let subagents = project.join(SESSION).join("subagents");
    std::fs::create_dir_all(&subagents).unwrap();
    let main = project.join(format!("{SESSION}.jsonl"));
    std::fs::write(&main, MAIN).unwrap();
    std::fs::write(subagents.join("agent-a1.jsonl"), A1).unwrap();
    std::fs::write(subagents.join("agent-a1.meta.json"), A1_META).unwrap();
    std::fs::write(subagents.join("agent-a2.jsonl"), A2).unwrap();
    std::fs::write(subagents.join("agent-a2.meta.json"), A2_META).unwrap();
    (Dir(dir), main)
}

fn salt() -> LateSalt {
    let cell = OnceLock::new();
    let _ = cell.set(Salt(SALT));
    Arc::new(cell)
}

fn call(tool: AgentTool, out: Option<&str>, cmd: Option<String>) -> AgentCall {
    let mut identity = AgentIdentity::shell();
    identity.tool = tool;
    identity.session = Some(SESSION.to_owned());
    identity.verified = true;
    identity.out_hash =
        out.map(|name| output_hash(&SALT, Path::new(&format!("{TASKS}/{name}.output"))));
    identity.cmd_hash = cmd;
    AgentCall {
        scope: Scope {
            op_id: Some(new_op_id()),
            actor: None,
        },
        identity,
        pid: Some(1),
    }
}

fn run(main: &Path, calls: &[AgentCall]) -> Vec<Event> {
    let sink = Arc::new(MemorySink::new());
    let sessions = Arc::new(Mutex::new(
        json!([{"id": SESSION, "transcriptPath": main.to_string_lossy()}]),
    ));
    let names = AgentNames::spawn(sink.clone(), salt(), sessions);
    for call in calls {
        names.enqueue(call, AT_MS);
    }
    names.close();
    sink.events()
}

fn text(event: &Event, name: &str) -> Option<String> {
    match event.fields.get(name)? {
        FieldValue::Name(t) => Some((*t).to_owned()),
        FieldValue::Token(t) => Some(t.to_string()),
        FieldValue::Ref { id, .. } => Some(id.to_string()),
        _ => None,
    }
}

#[test]
fn a_background_commands_output_file_names_the_subagent_and_no_label_text_is_logged() {
    let (_dir, main) = fixture("one");
    let events = run(&main, &[call(AgentTool::Claude, Some("ba1000001"), None)]);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].spec.name, "agent.named");
    assert_eq!(text(&events[0], "asker").as_deref(), Some("subagent"));
    assert_eq!(text(&events[0], "agent").as_deref(), Some("a1"));
    assert!(!format!("{:?}", events[0].fields).contains("Port the parser"));
    let main_events = run(&main, &[call(AgentTool::Claude, Some("bmain0001"), None)]);
    assert_eq!(text(&main_events[0], "asker").as_deref(), Some("main"));
}

#[test]
fn two_matches_are_unknown() {
    let (_dir, main) = fixture("two");
    let argv: Vec<String> = [
        "/Applications/Notchling.app/Contents/MacOS/notchling",
        "ask",
        "--choice",
        "Keep it",
        "--choice",
        "Drop it",
        "--",
        "Keep the old API?",
    ]
    .iter()
    .map(|w| w.to_string())
    .collect();
    let cmd = command_hash(&SALT, &argv);
    let events = run(&main, &[call(AgentTool::Claude, None, cmd)]);
    assert_eq!(text(&events[0], "asker").as_deref(), Some("unknown"));
    assert_eq!(text(&events[0], "agent"), None);
}

#[test]
fn a_burst_of_calls_from_one_session_resolves_each_from_its_own_evidence() {
    let (_dir, main) = fixture("burst");
    let calls: Vec<AgentCall> = (0..100)
        .map(|i| {
            let out = if i % 2 == 0 { "ba1000001" } else { "ba2000002" };
            call(AgentTool::Claude, Some(out), None)
        })
        .collect();
    let events = run(&main, &calls);
    assert_eq!(events.len(), 100);
    for (i, event) in events.iter().enumerate() {
        let want = if i % 2 == 0 { "a1" } else { "a2" };
        assert_eq!(text(event, "agent").as_deref(), Some(want), "call {i}");
    }
}

#[test]
fn a_delegated_tool_is_labelled_by_its_tool_and_a_shell_is_unknown() {
    let (_dir, main) = fixture("tools");
    let events = run(
        &main,
        &[
            call(AgentTool::Codex, None, None),
            call(AgentTool::Antigravity, None, None),
            call(AgentTool::Shell, None, None),
        ],
    );
    let askers: Vec<Option<String>> = events.iter().map(|e| text(e, "asker")).collect();
    assert_eq!(
        askers,
        [
            Some("codex".to_owned()),
            Some("antigravity".to_owned()),
            Some("unknown".to_owned())
        ]
    );
}

#[test]
fn a_transcript_gone_gives_unknown() {
    let (dir, main) = fixture("gone");
    let events = {
        std::fs::remove_dir_all(&dir.0).unwrap();
        run(&main, &[call(AgentTool::Claude, Some("ba1000001"), None)])
    };
    assert_eq!(text(&events[0], "asker").as_deref(), Some("unknown"));
}
