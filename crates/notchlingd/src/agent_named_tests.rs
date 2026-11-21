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
use std::time::{Duration, Instant};

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

fn waiting_names(
    main: &Path,
    retries: Vec<Duration>,
) -> (AgentNames, Arc<MemorySink>, Arc<Mutex<serde_json::Value>>) {
    let sink = Arc::new(MemorySink::new());
    let sessions = Arc::new(Mutex::new(
        json!([{"id": SESSION, "transcriptPath": main.to_string_lossy()}]),
    ));
    let names = AgentNames::spawn_with(sink.clone(), salt(), sessions.clone(), retries);
    (names, sink, sessions)
}

fn wait_for(sink: &MemorySink, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while sink.events().len() < count && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_label_not_in_the_transcript_yet_is_found_on_a_retry_not_stamped_unknown() {
    let (_dir, main) = fixture("retry");
    let late = main.with_file_name("late.jsonl");
    let (names, sink, sessions) = waiting_names(&late, vec![Duration::from_millis(250); 4]);
    names.enqueue(&call(AgentTool::Claude, Some("ba1000001"), None), AT_MS);
    std::thread::sleep(Duration::from_millis(80));
    assert!(sink.events().is_empty());
    *sessions.lock().unwrap() = json!([{"id": SESSION, "transcriptPath": main.to_string_lossy()}]);
    wait_for(&sink, 1);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(text(&events[0], "asker").as_deref(), Some("subagent"));
    assert_eq!(text(&events[0], "agent").as_deref(), Some("a1"));
    names.close();
    assert_eq!(sink.events().len(), 1);
}

#[test]
fn a_call_that_never_resolves_is_unknown_once_after_its_retries() {
    let (_dir, main) = fixture("exhaust");
    let gone = main.with_file_name("gone.jsonl");
    let (names, sink, _) = waiting_names(&gone, vec![Duration::from_millis(15); 2]);
    names.enqueue(&call(AgentTool::Claude, Some("ba1000001"), None), AT_MS);
    wait_for(&sink, 1);
    std::thread::sleep(Duration::from_millis(120));
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(text(&events[0], "asker").as_deref(), Some("unknown"));
    names.close();
}

#[test]
fn closing_settles_the_calls_still_waiting_without_waiting_for_the_delays() {
    let (_dir, main) = fixture("close");
    let gone = main.with_file_name("gone.jsonl");
    let (names, sink, _) = waiting_names(&gone, vec![Duration::from_secs(60)]);
    names.enqueue(&call(AgentTool::Claude, Some("ba1000001"), None), AT_MS);
    std::thread::sleep(Duration::from_millis(60));
    assert!(sink.events().is_empty());
    let started = Instant::now();
    names.close();
    assert!(started.elapsed() < Duration::from_secs(5));
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(text(&events[0], "asker").as_deref(), Some("unknown"));
}

#[test]
fn an_unverified_session_claim_is_never_looked_up() {
    let (_dir, main) = fixture("unverified");
    let mut claimed = call(AgentTool::Claude, Some("ba1000001"), None);
    claimed.identity.verified = false;
    let events = run(&main, &[claimed]);
    assert_eq!(text(&events[0], "asker").as_deref(), Some("unknown"));
    assert_eq!(text(&events[0], "agent"), None);
}
