use super::*;
use crate::delegate::test_support::{rig, Rig};
use crate::tools::acp::fake::{replaying, FakeHandle, FakeSpawner, Responder};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const RECORDING: &str =
    include_str!("../../../../legacy/macos/Tests/NotchlingTests/Fixtures/acp-agy.jsonl");

fn config() -> Value {
    json!([
        {"id": "model", "name": "Model", "type": "select", "currentValue": "gemini-4-argon",
         "options": [{"value": "gemini-4-argon", "name": "G"}, {"value": "claude-opus-5-5", "name": "C"}]},
        {"id": "thought_level", "name": "Thinking", "type": "select", "currentValue": "medium",
         "options": [{"value": "low"}, {"value": "medium"}, {"value": "high"}]},
        {"id": "mode", "name": "Mode", "type": "select", "currentValue": "default",
         "options": [{"value": "default"}, {"value": "accept-edits"}]},
    ])
}

fn update(h: &FakeHandle, session: &str, update: Value) {
    h.push(&json!({"jsonrpc": "2.0", "method": "session/update",
        "params": {"sessionId": session, "update": update}}));
}

fn chunk(h: &FakeHandle, text: &str) {
    update(
        h,
        "sess-1",
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}),
    );
}

fn answer(h: &FakeHandle, message: &Value, result: Value) {
    h.push(&json!({"jsonrpc": "2.0", "id": message["id"], "result": result}));
}

type OnPrompt = Arc<dyn Fn(&FakeHandle, &Value) + Send + Sync>;

fn agent(on_prompt: OnPrompt) -> Responder {
    Arc::new(move |m: &Value, h: &FakeHandle| {
        let Some(method) = m.get("method").and_then(Value::as_str) else {
            return;
        };
        match method {
            "initialize" => answer(h, m, json!({"protocolVersion": 1})),
            "session/new" => answer(
                h,
                m,
                json!({"sessionId": "sess-1", "configOptions": config()}),
            ),
            "session/load" => {
                chunk(h, "OLD history");
                answer(
                    h,
                    m,
                    json!({"sessionId": m["params"]["sessionId"], "configOptions": config()}),
                );
            }
            "session/set_config_option" => answer(h, m, json!({})),
            "session/prompt" => on_prompt(h, m),
            _ => {}
        }
    })
}

fn plain_turn() -> OnPrompt {
    Arc::new(|h, m| {
        chunk(h, "Let me look. ");
        update(
            h,
            "sess-1",
            json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "name": "read_file"}),
        );
        chunk(h, "Final ");
        chunk(h, "answer.");
        answer(
            h,
            m,
            json!({"stopReason": "end_turn", "usage": {"totalTokens": 42}}),
        );
    })
}

fn rig_with(responder: Responder) -> Rig {
    rig(FakeSpawner::with_responder(responder))
}

fn run_it(rig: &Rig, options: &Options) -> (Outcome, String) {
    let mut progress = Vec::new();
    let outcome = run(options, &rig.deps, &mut progress);
    (outcome, String::from_utf8(progress).unwrap())
}

fn set_options(h: &FakeHandle) -> Vec<(String, String)> {
    h.requests("session/set_config_option")
        .iter()
        .map(|r| {
            (
                r["params"]["configId"].as_str().unwrap().to_string(),
                r["params"]["value"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn a_read_only_turn_picks_the_model_and_level_and_keeps_the_text_after_the_last_tool_call() {
    let rig = rig_with(agent(plain_turn()));
    let mut options = rig.options(Tool::Agy, "review this");
    options.model = Some("claude-opus-5-5-high".into());
    let (outcome, progress) = run_it(&rig, &options);

    assert_eq!(outcome.error, None);
    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.reply, "Final answer.");
    assert_eq!(outcome.session.as_deref(), Some("sess-1"));
    assert_eq!(outcome.tokens, Some(42));
    assert_eq!(outcome.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(outcome.effort.as_deref(), Some("high"));
    assert!(progress.is_empty());

    let h = rig.agy.spawned().remove(0);
    let init = &h.requests("initialize")[0];
    assert_eq!(init["params"]["clientCapabilities"]["terminal"], false);
    assert_eq!(
        init["params"]["clientCapabilities"]["fs"]["writeTextFile"],
        false
    );
    let new = &h.requests("session/new")[0];
    assert_eq!(new["params"]["cwd"], options.folder);
    assert_eq!(
        set_options(&h),
        [
            ("model".into(), "claude-opus-5-5".into()),
            ("thought_level".into(), "high".into())
        ],
        "no accept-edits without --write"
    );
    let prompt = &h.requests("session/prompt")[0];
    assert_eq!(prompt["params"]["prompt"][0]["text"], "review this");
    assert!(h.killed(), "the process is closed at the end");
    let (program, args, folder) = rig.agy.calls().remove(0);
    assert_eq!(
        (program.as_str(), args, folder.as_str()),
        (
            "/fake/agy",
            vec!["acp".to_string()],
            options.folder.as_str()
        )
    );
}

#[test]
fn the_defaults_are_reported_when_nothing_was_picked() {
    let rig = rig_with(agent(plain_turn()));
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "x"));
    assert_eq!(outcome.model.as_deref(), Some("gemini-4-argon"));
    assert_eq!(outcome.effort.as_deref(), Some("medium"));
    assert!(set_options(&rig.agy.spawned()[0]).is_empty());
}

fn asking(kind: &'static str, title: &'static str, id: Value) -> OnPrompt {
    Arc::new(move |h, m| {
        h.push(
            &json!({"jsonrpc": "2.0", "id": id, "method": "session/request_permission",
            "params": {"sessionId": "sess-1",
                "toolCall": {"toolCallId": "c", "kind": kind, "title": title},
                "options": [
                    {"optionId": "always", "name": "Always", "kind": "allow_always"},
                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "deny", "name": "Deny", "kind": "reject_once"}]}}),
        );
        chunk(h, "Done.");
        answer(h, m, json!({"stopReason": "end_turn"}));
    })
}

fn replies_to(h: &FakeHandle, id: &Value) -> Vec<Value> {
    h.written()
        .into_iter()
        .filter(|m| m.get("id") == Some(id) && m.get("method").is_none())
        .collect()
}

#[test]
fn with_write_an_edit_is_allowed_and_accept_edits_is_set() {
    let rig = rig_with(agent(asking("edit", "File a.txt", json!("p1"))));
    let mut options = rig.options(Tool::Agy, "change it");
    options.write = true;
    let (outcome, _) = run_it(&rig, &options);
    let h = rig.agy.spawned().remove(0);
    assert_eq!(set_options(&h), [("mode".into(), "accept-edits".into())]);
    assert_eq!(
        replies_to(&h, &json!("p1")),
        [json!({"jsonrpc": "2.0", "id": "p1", "result":
            {"outcome": {"outcome": "selected", "optionId": "allow"}}})],
        "the allow_once option, with the id as it came"
    );
    assert!(outcome.declined.is_empty());
    assert_eq!(outcome.reply, "Done.");
}

#[test]
fn without_write_even_an_edit_is_declined_and_listed() {
    let rig = rig_with(agent(asking("edit", "File a.txt", json!(7))));
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "x"));
    let h = rig.agy.spawned().remove(0);
    assert_eq!(
        replies_to(&h, &json!(7))[0]["result"]["outcome"]["optionId"],
        "deny"
    );
    assert_eq!(outcome.declined, ["File a.txt"]);
}

#[test]
fn with_write_only_edits_are_allowed_other_requests_are_declined() {
    let rig = rig_with(agent(asking("execute", "Run rm -rf", json!("p2"))));
    let mut options = rig.options(Tool::Agy, "x");
    options.write = true;
    let (outcome, _) = run_it(&rig, &options);
    let h = rig.agy.spawned().remove(0);
    assert_eq!(
        replies_to(&h, &json!("p2"))[0]["result"]["outcome"]["optionId"],
        "deny"
    );
    assert_eq!(outcome.declined, ["Run rm -rf"]);
}

#[test]
fn a_request_with_no_reject_option_is_cancelled_and_other_calls_are_refused() {
    let on_prompt: OnPrompt = Arc::new(|h, m| {
        h.push(
            &json!({"jsonrpc": "2.0", "id": 3, "method": "session/request_permission",
            "params": {"toolCall": {"kind": "execute"}, "options": []}}),
        );
        h.push(
            &json!({"jsonrpc": "2.0", "id": "fs1", "method": "fs/read_text_file",
            "params": {"path": "/etc/passwd"}}),
        );
        answer(h, m, json!({"stopReason": "end_turn"}));
    });
    let rig = rig_with(agent(on_prompt));
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "x"));
    let h = rig.agy.spawned().remove(0);
    assert_eq!(
        replies_to(&h, &json!(3))[0]["result"]["outcome"]["outcome"],
        "cancelled"
    );
    let refused = &replies_to(&h, &json!("fs1"))[0];
    assert_eq!(refused["error"]["code"], -32601);
    assert_eq!(
        outcome.declined,
        ["execute"],
        "the kind stands in for a missing title"
    );
}

#[test]
fn continuing_a_session_loads_it_and_ignores_the_replayed_history() {
    let on_prompt: OnPrompt = Arc::new(|h, m| {
        answer(h, m, json!({"stopReason": "end_turn"}));
    });
    let rig = rig_with(agent(on_prompt));
    let mut options = rig.options(Tool::Agy, "and then?");
    options.session = Some("conv-9".into());
    let (outcome, _) = run_it(&rig, &options);
    let h = rig.agy.spawned().remove(0);
    let load = &h.requests("session/load")[0];
    assert_eq!(load["params"]["sessionId"], "conv-9");
    assert_eq!(load["params"]["cwd"], options.folder);
    assert!(h.requests("session/new").is_empty());
    assert_eq!(outcome.session.as_deref(), Some("conv-9"));
    assert_eq!(outcome.reply, "", "the old history is not the reply");
}

#[test]
fn a_model_that_is_not_offered_fails_before_prompting() {
    let rig = rig_with(agent(plain_turn()));
    let mut options = rig.options(Tool::Agy, "x");
    options.model = Some("nope".into());
    let (outcome, _) = run_it(&rig, &options);
    assert_eq!(
        outcome.error.as_deref(),
        Some("--model nope isn't offered. Choices: gemini-4-argon, claude-opus-5-5.")
    );
    assert_eq!(outcome.exit_code(), 1);
    let h = rig.agy.spawned().remove(0);
    assert!(h.requests("session/prompt").is_empty());
    assert!(h.killed());

    let mut options = rig.options(Tool::Agy, "x");
    options.effort = Some("medium".into());
    options.effort = Some("turbo".into());
    let (outcome, _) = run_it(&rig, &options);
    assert!(outcome
        .error
        .unwrap()
        .starts_with("--effort turbo isn't offered. Choices: low, medium, high."));
}

#[test]
fn a_turn_that_does_not_end_normally_fails_with_its_reason() {
    let on_prompt: OnPrompt = Arc::new(|h, m| answer(h, m, json!({"stopReason": "refusal"})));
    let rig = rig_with(agent(on_prompt));
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "x"));
    assert_eq!(
        outcome.error.as_deref(),
        Some("Antigravity stopped (refusal).")
    );
}

#[test]
fn the_timeout_cancels_the_turn_and_exits_124() {
    let on_prompt: OnPrompt = Arc::new(|h, _| chunk(h, "partial"));
    let rig = rig_with(agent(on_prompt));
    let mut options = rig.options(Tool::Agy, "slow");
    options.timeout = Duration::from_millis(200);
    let (outcome, _) = run_it(&rig, &options);
    assert!(outcome.timed_out);
    assert_eq!(outcome.exit_code(), 124);
    assert_eq!(outcome.error.as_deref(), Some("Stopped after 0 s."));
    let h = rig.agy.spawned().remove(0);
    let cancel = &h.requests("session/cancel")[0];
    assert_eq!(cancel["params"]["sessionId"], "sess-1");
    assert!(cancel.get("id").is_none(), "a notification");
    assert!(h.killed());
    assert_eq!(outcome.reply, "partial", "what it said so far");
}

#[test]
fn a_stop_signal_cancels_the_turn_and_says_so() {
    let on_prompt: OnPrompt = Arc::new(|_, _| {});
    let rig = rig_with(agent(on_prompt));
    let stop = rig.deps.stop.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        stop.store(true, Ordering::SeqCst);
    });
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "slow"));
    stopper.join().unwrap();
    assert!(!outcome.timed_out);
    assert_eq!(outcome.error.as_deref(), Some("Stopped."));
    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(rig.agy.spawned()[0].requests("session/cancel").len(), 1);
}

#[test]
fn a_process_that_dies_mid_turn_fails_the_run_with_its_last_line() {
    let on_prompt: OnPrompt = Arc::new(|h, _| h.die(Some("panic: out of memory")));
    let rig = rig_with(agent(on_prompt));
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "x"));
    assert_eq!(outcome.error.as_deref(), Some("panic: out of memory"));
    assert_eq!(outcome.exit_code(), 1);
}

#[test]
fn framing_faults_do_not_stop_the_turn() {
    let on_prompt: OnPrompt = Arc::new(|h, m| {
        h.push_line("this is not json");
        h.push_oversize();
        h.push(&json!({"jsonrpc": "2.0", "id": 999, "result": {}}));
        chunk(h, "Fine.");
        answer(h, m, json!({"stopReason": "end_turn"}));
    });
    let rig = rig_with(agent(on_prompt));
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "x"));
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.reply, "Fine.");
}

#[test]
fn without_the_binary_or_when_it_will_not_start_it_fails_at_once() {
    let mut rig = rig_with(agent(plain_turn()));
    rig.agy.refuse_with("permission denied");
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "x"));
    assert_eq!(outcome.error.as_deref(), Some("permission denied"));
    rig.deps.agy_binary = None;
    let (outcome, _) = run_it(&rig, &rig.options(Tool::Agy, "x"));
    assert_eq!(
        outcome.error.as_deref(),
        Some("Couldn't find the agy command. Install Antigravity CLI first.")
    );
    assert_eq!(outcome.session, None);
}

fn brain_session(media_root: &Path) -> PathBuf {
    media_root.join("sess-1")
}

fn subagent_files(media_root: &Path, answered: bool) {
    let system = brain_session(media_root).join(".system_generated");
    for sub in ["subagents", "messages", "logs"] {
        std::fs::create_dir_all(system.join(sub)).unwrap();
    }
    std::fs::write(
        system.join("subagents/sub1.json"),
        r#"{"conversationId":"sub1"}"#,
    )
    .unwrap();
    std::fs::write(system.join("messages/m1.json"), r#"{"sender":"sub1"}"#).unwrap();
    let mut log = String::from(
        r#"{"type":"PLANNER_RESPONSE","status":"DONE","content":"I asked the generator."}"#,
    );
    log.push('\n');
    log.push_str(r#"{"type":"SYSTEM_MESSAGE","status":"DONE","content":"x"}"#);
    if answered {
        log.push('\n');
        log.push_str(
            r#"{"type":"PLANNER_RESPONSE","status":"DONE","content":"Saved: /x/teapot.jpg"}"#,
        );
    }
    std::fs::write(system.join("logs/transcript.jsonl"), log).unwrap();
}

#[test]
fn the_parents_answer_after_its_subagents_is_the_reply_and_their_media_is_picked_up() {
    let root_for_agent = Arc::new(Mutex::new(PathBuf::new()));
    let seen = root_for_agent.clone();
    let on_prompt: OnPrompt = Arc::new(move |h, m| {
        chunk(h, "Asked the generator.");
        let media_root = seen.lock().unwrap().clone();
        subagent_files(&media_root, true);
        for (dir, name) in [("sess-1", "main.png"), ("sub1", "teapot.jpg")] {
            let folder = media_root.join(dir);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(name), "x").unwrap();
        }
        answer(h, m, json!({"stopReason": "end_turn"}));
    });
    let rig = rig_with(agent(on_prompt));
    *root_for_agent.lock().unwrap() = rig.agy_media();
    let (outcome, progress) = run_it(&rig, &rig.options(Tool::Agy, "draw a teapot"));
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.reply, "Saved: /x/teapot.jpg");
    assert!(progress.is_empty(), "answered at once: nothing to wait for");
    let mut media = outcome.media.clone();
    media.sort();
    assert_eq!(
        media,
        [
            rig.agy_media().join("sess-1/main.png").to_string_lossy(),
            rig.agy_media().join("sub1/teapot.jpg").to_string_lossy(),
        ]
    );
}

#[test]
fn the_run_waits_for_a_subagent_that_is_still_working() {
    let root_for_agent = Arc::new(Mutex::new(PathBuf::new()));
    let seen = root_for_agent.clone();
    let on_prompt: OnPrompt = Arc::new(move |h, m| {
        let media_root = seen.lock().unwrap().clone();
        let system = brain_session(&media_root).join(".system_generated");
        for sub in ["subagents", "messages", "logs"] {
            std::fs::create_dir_all(system.join(sub)).unwrap();
        }
        std::fs::write(
            system.join("subagents/sub1.json"),
            r#"{"conversationId":"sub1"}"#,
        )
        .unwrap();
        let later = media_root.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            subagent_files(&later, true);
        });
        answer(h, m, json!({"stopReason": "end_turn"}));
    });
    let rig = rig_with(agent(on_prompt));
    *root_for_agent.lock().unwrap() = rig.agy_media();
    let (outcome, progress) = run_it(&rig, &rig.options(Tool::Agy, "draw"));
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.reply, "Saved: /x/teapot.jpg");
    assert_eq!(progress, "Waiting for Antigravity's subagents (1)…\n");
}

#[test]
fn the_timeout_also_ends_the_wait_for_subagents() {
    let root_for_agent = Arc::new(Mutex::new(PathBuf::new()));
    let seen = root_for_agent.clone();
    let on_prompt: OnPrompt = Arc::new(move |h, m| {
        let system = brain_session(&seen.lock().unwrap()).join(".system_generated");
        std::fs::create_dir_all(system.join("subagents")).unwrap();
        std::fs::write(
            system.join("subagents/sub1.json"),
            r#"{"conversationId":"sub1"}"#,
        )
        .unwrap();
        answer(h, m, json!({"stopReason": "end_turn"}));
    });
    let rig = rig_with(agent(on_prompt));
    *root_for_agent.lock().unwrap() = rig.agy_media();
    let mut options = rig.options(Tool::Agy, "draw");
    options.timeout = Duration::from_millis(400);
    let (outcome, progress) = run_it(&rig, &options);
    assert!(outcome.timed_out, "{outcome:?}");
    assert_eq!(outcome.exit_code(), 124);
    assert!(progress.contains("Waiting for Antigravity's subagents (1)"));
}

fn recorded() -> Responder {
    let replay = replaying(RECORDING);
    Arc::new(move |m: &Value, h: &FakeHandle| {
        if m.get("method").and_then(Value::as_str) == Some("session/set_config_option") {
            answer(h, m, json!({}));
        } else {
            replay(m, h);
        }
    })
}

#[test]
fn it_runs_against_the_real_recording_allowing_the_edit_only_with_write() {
    let rig = rig_with(recorded());
    let mut options = rig.options(Tool::Agy, "Change the text in hello.txt");
    options.session = Some("d67413c2-a0f3-4381-8a8e-e98b541d6d76".into());
    options.write = true;
    let (outcome, _) = run_it(&rig, &options);
    assert_eq!(outcome.error, None, "{outcome:?}");
    assert_eq!(outcome.reply, "ok", "not the history's earlier replies");
    assert_eq!(outcome.tokens, Some(16605));
    assert!(outcome.declined.is_empty());
    let h = rig.agy.spawned().remove(0);
    assert_eq!(
        replies_to(&h, &json!("1"))[0]["result"]["outcome"]["optionId"],
        "allow"
    );
    assert_eq!(set_options(&h), [("mode".into(), "accept-edits".into())]);

    let read_only = rig_with(recorded());
    let mut options = read_only.options(Tool::Agy, "Change the text in hello.txt");
    options.session = Some("d67413c2-a0f3-4381-8a8e-e98b541d6d76".into());
    let (outcome, _) = run_it(&read_only, &options);
    assert_eq!(outcome.reply, "ok");
    assert_eq!(outcome.declined, ["File hello.txt"]);
    let h = read_only.agy.spawned().remove(0);
    assert_eq!(
        replies_to(&h, &json!("1"))[0]["result"]["outcome"]["optionId"],
        "deny"
    );
}

#[test]
fn a_recorded_agy_job_names_its_session_and_model_in_its_record() {
    use crate::delegate::record::{self, Delegation, End, Recorder};
    let mut rig = rig_with(agent(plain_turn()));
    let dir = rig.root.join("data/delegations");
    let mut options = rig.options(Tool::Agy, "Second opinion\nplease");
    options.model = Some("claude-opus-5-5".into());
    options.effort = Some("high".into());
    let start = Delegation::new("parent-1", "/work", &options, 10.0, std::process::id());
    rig.deps.recorder = Some(Arc::new(Recorder::start(&dir, start, 10.0)));
    assert_eq!(
        record::read_all(&dir)[0].file_name(),
        format!("agy-pending-{}.json", std::process::id()),
        "listed at once, before agy names the session"
    );
    let (outcome, _) = run_it(&rig, &options);
    assert_eq!(outcome.error, None);
    let records = record::read_all(&dir);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].file_name(), "agy-sess-1.json");
    assert_eq!(records[0].model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(records[0].effort.as_deref(), Some("high"));
    assert_eq!(
        records[0].ended, None,
        "the command's own end is set by command::run"
    );
    rig.deps
        .recorder
        .as_ref()
        .unwrap()
        .finish(&outcome, false, 20.0);
    assert_eq!(record::read_all(&dir)[0].outcome, Some(End::Ok));
}
