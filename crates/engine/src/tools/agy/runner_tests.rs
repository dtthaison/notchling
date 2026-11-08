use super::*;
use crate::tools::acp::fake::{FakeHandle, FakeSpawner};
use crate::transcript::model::ItemKind;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

const RECORDING: &str =
    include_str!("../../../../../legacy/macos/Tests/NotchlingTests/Fixtures/acp-agy.jsonl");
const SESSION: &str = "d67413c2-a0f3-4381-8a8e-e98b541d6d76";

#[derive(Default)]
struct Events(Mutex<Vec<(String, Change)>>);

impl Sink for Events {
    fn changed(&self, session: &str, change: Change) {
        self.0.lock().unwrap().push((session.to_string(), change));
    }
}

impl Events {
    fn all(&self) -> Vec<Change> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(_, c)| c.clone())
            .collect()
    }

    /// Waits for a change matching `pred`; panics after ten seconds.
    fn wait(&self, what: &str, pred: impl Fn(&Change) -> bool) -> Change {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(found) = self.all().into_iter().find(|c| pred(c)) {
                return found;
            }
            assert!(
                Instant::now() < end,
                "timed out waiting for {what}: {:?}",
                self.all()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn count(&self, pred: impl Fn(&Change) -> bool) -> usize {
        self.all().iter().filter(|c| pred(c)).count()
    }
}

struct Rig {
    runner: Runner,
    events: Arc<Events>,
    spawner: Arc<FakeSpawner>,
    clock: Arc<AtomicU64>,
}

fn rig_with(spawner: FakeSpawner, binary: Option<&str>) -> Rig {
    let spawner = Arc::new(spawner);
    let events = Arc::new(Events::default());
    let clock = Arc::new(AtomicU64::new(1_000));
    let c = clock.clone();
    let runner = Runner::new(
        spawner.clone(),
        binary.map(String::from),
        events.clone(),
        Arc::new(move || c.load(Ordering::SeqCst) as f64),
        Timing::default(),
    );
    Rig {
        runner,
        events,
        spawner,
        clock,
    }
}

fn rig() -> Rig {
    rig_with(FakeSpawner::replaying(RECORDING), Some("/fake/agy"))
}

fn loaded(rig: &Rig) -> FakeHandle {
    rig.runner.open(SESSION, "/work", false).unwrap();
    rig.events.wait("Loaded", |c| *c == Change::Loaded);
    rig.spawner.spawned().remove(0)
}

fn ok(process: &FakeHandle, request: &Value, result: Value) {
    process.push(&json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
}

#[test]
fn opening_a_session_initializes_without_fs_or_terminal_and_loads_the_history() {
    let rig = rig();
    let process = loaded(&rig);
    let (program, args, folder) = rig.spawner.calls().remove(0);
    assert_eq!(
        (program.as_str(), args, folder.as_str()),
        ("/fake/agy", vec!["acp".to_string()], "/work")
    );
    let init = &process.requests("initialize")[0];
    assert_eq!(init["params"]["clientCapabilities"]["terminal"], false);
    assert_eq!(
        init["params"]["clientCapabilities"]["fs"]["readTextFile"],
        false
    );
    assert_eq!(
        init["params"]["clientCapabilities"]["fs"]["writeTextFile"],
        false
    );
    let load = &process.requests("session/load")[0];
    assert_eq!(load["params"]["sessionId"], SESSION);
    assert_eq!(load["params"]["cwd"], "/work");
    assert_eq!(load["params"]["mcpServers"], json!([]));

    let r = &rig.runner;
    assert!(r.is_open(SESSION) && !r.is_loading(SESSION) && !r.is_working(SESSION));
    assert_eq!(
        r.title(SESSION).as_deref(),
        None,
        "the title comes with the first prompt in this recording"
    );
    let c = r.conversation(SESSION).unwrap();
    assert!(matches!(c.items[0].kind, ItemKind::User { .. }));
    assert!(c.items.iter().any(|i| matches!(i.kind, ItemKind::Tools(_))));
    assert!(
        !r.config(SESSION).is_empty(),
        "the load result lists Model, Thinking Level, Mode"
    );
    assert_eq!(r.folder(SESSION).as_deref(), Some("/work"));
    // A second open of an open session starts no second process.
    r.open(SESSION, "/work", false).unwrap();
    assert_eq!(rig.spawner.spawned().len(), 1);
}

#[test]
fn a_prompt_runs_a_turn_holds_the_permission_and_never_approves_it_itself() {
    let rig = rig();
    let process = loaded(&rig);
    let r = &rig.runner;
    r.prompt(SESSION, "Change the text in hello.txt", "/work")
        .unwrap();
    assert!(r.is_working(SESSION));
    assert_eq!(
        r.prompt(SESSION, "again", "/work"),
        Err(RunError::Busy),
        "one turn at a time"
    );
    let Change::PermissionPending(p) = rig.events.wait("a permission", |c| {
        matches!(c, Change::PermissionPending(_))
    }) else {
        unreachable!()
    };
    assert_eq!(p.rpc_id, json!("1"), "a string id, as it came");
    assert_eq!(p.call.name, "edit_file");
    assert_eq!(
        p.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["allow", "deny"]
    );
    // Nothing answered it: no response written, the turn is still open.
    std::thread::sleep(Duration::from_millis(100));
    assert!(process.written().iter().all(|m| m.get("method").is_some()));
    assert!(r.is_working(SESSION));
    assert_eq!(r.permission(SESSION).unwrap().rpc_text(), "1");

    // Wrong id, wrong option: refused, and the permission still waits.
    assert!(matches!(
        r.answer(SESSION, "2", Some("allow")),
        Err(RunError::Refused(_))
    ));
    assert!(matches!(
        r.answer(SESSION, "1", Some("always-yes")),
        Err(RunError::Refused(_))
    ));
    assert!(r.permission(SESSION).is_some());

    assert_eq!(r.answer(SESSION, "1", Some("allow")).unwrap(), "allow_once");
    let answer = process
        .written()
        .into_iter()
        .find(|m| m.get("method").is_none())
        .unwrap();
    assert_eq!(answer["id"], json!("1"));
    assert_eq!(
        answer["result"]["outcome"],
        json!({"outcome": "selected", "optionId": "allow"})
    );
    rig.events
        .wait("the turn's end", |c| matches!(c, Change::TurnEnded(_)));
    assert!(
        matches!(
            r.answer(SESSION, "1", Some("allow")),
            Err(RunError::Refused(_))
        ),
        "answered once only"
    );

    let events = rig.events.all();
    let at = |f: &dyn Fn(&Change) -> bool| events.iter().position(f).unwrap();
    assert!(
        at(&|c| *c == Change::TurnStarted) < at(&|c| matches!(c, Change::PermissionPending(_)))
    );
    assert!(
        at(&|c| matches!(c, Change::PermissionPending(_)))
            < at(&|c| matches!(c, Change::PermissionResolved { .. }))
    );
    assert!(
        at(&|c| matches!(c, Change::PermissionResolved { .. }))
            < at(&|c| matches!(c, Change::TurnEnded(None)))
    );
    let c = r.conversation(SESSION).unwrap();
    assert!(!r.is_working(SESSION));
    assert!(matches!(
        &c.items.last().unwrap().kind,
        ItemKind::TurnEnd { .. }
    ));
    assert!(c
        .items
        .iter()
        .any(|i| matches!(&i.kind, ItemKind::Assistant { text } if text == "ok")));
    let sent = &process.requests("session/prompt")[0];
    assert_eq!(
        sent["params"]["prompt"][0],
        json!({"type": "text", "text": "Change the text in hello.txt"})
    );
    // The next turn may go.
    assert!(r.prompt(SESSION, "next", "/work").is_ok());
}

#[test]
fn stopping_answers_a_waiting_permission_as_cancelled_then_cancels_the_turn() {
    let rig = rig();
    let process = loaded(&rig);
    rig.runner.prompt(SESSION, "edit", "/work").unwrap();
    rig.events.wait("a permission", |c| {
        matches!(c, Change::PermissionPending(_))
    });
    rig.runner.cancel(SESSION);
    let sent: Vec<Value> = process.written().into_iter().skip(3).collect();
    assert_eq!(sent[0]["id"], json!("1"));
    assert_eq!(
        sent[0]["result"]["outcome"],
        json!({"outcome": "cancelled"})
    );
    assert_eq!(sent[1]["method"], "session/cancel");
    assert_eq!(sent[1]["params"]["sessionId"], SESSION);
    assert!(sent[1].get("id").is_none(), "a notification");
    assert_eq!(
        rig.events.count(
            |c| matches!(c, Change::PermissionResolved { outcome, .. } if outcome == "cancelled")
        ),
        1
    );
    // Cancel on an idle session does nothing.
    let idle = rig_with(FakeSpawner::replaying(RECORDING), Some("/fake/agy"));
    let p = loaded(&idle);
    let before = p.written().len();
    idle.runner.cancel(SESSION);
    assert_eq!(p.written().len(), before);
}

#[test]
fn calls_from_the_agent_other_than_a_permission_get_32601_whatever_their_id() {
    let rig = rig();
    let process = loaded(&rig);
    process.push(&json!({"jsonrpc":"2.0","id":"fs-1","method":"fs/read_text_file","params":{"path":"/etc/passwd"}}));
    process.push(&json!({"jsonrpc":"2.0","id":9,"method":"terminal/create","params":{}}));
    process.push(&json!({"jsonrpc":"2.0","id":"p","method":"session/request_permission","params":{"options":[]}}));
    let end = Instant::now() + Duration::from_secs(10);
    let errors = loop {
        let errors: Vec<Value> = process
            .written()
            .into_iter()
            .filter(|m| m.get("error").is_some())
            .collect();
        if errors.len() == 3 || Instant::now() > end {
            break errors;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let ids: Vec<&Value> = errors.iter().map(|e| &e["id"]).collect();
    assert_eq!(ids, [&json!("fs-1"), &json!(9), &json!("p")]);
    assert!(errors.iter().all(|e| e["error"]["code"] == -32601));
    assert!(
        rig.runner.permission(SESSION).is_none(),
        "a permission without a tool call isn't one"
    );
}

#[test]
fn garbage_on_the_wire_does_not_stop_the_session() {
    let rig = rig();
    let process = loaded(&rig);
    process.push_line("not json {");
    process.push_oversize();
    process.push(&json!({"jsonrpc":"2.0","id":4242,"result":{}}));
    process.push(&json!({"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"still here"}}}}));
    let end = Instant::now() + Duration::from_secs(10);
    while !rig
        .runner
        .conversation(SESSION)
        .unwrap()
        .items
        .iter()
        .any(|i| matches!(&i.kind, ItemKind::Assistant { text } if text.ends_with("still here")))
    {
        assert!(
            Instant::now() < end,
            "the update after the garbage never arrived"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(rig.runner.is_open(SESSION) && !process.killed());
}

#[test]
fn a_process_that_dies_mid_turn_ends_the_turn_with_its_error_and_the_session_reopens() {
    let rig = rig();
    let process = loaded(&rig);
    rig.runner.prompt(SESSION, "edit", "/work").unwrap();
    rig.events.wait("a permission", |c| {
        matches!(c, Change::PermissionPending(_))
    });
    process.die(Some("agy: signal 9"));
    rig.events.wait("Closed", |c| *c == Change::Closed);
    let events = rig.events.all();
    assert!(events.contains(&Change::TurnEnded(Some("agy: signal 9".into()))));
    assert!(events.iter().any(
        |c| matches!(c, Change::PermissionResolved { outcome, .. } if outcome == "cancelled")
    ));
    assert!(!rig.runner.is_open(SESSION));
    assert!(!rig.runner.is_working(SESSION));
    // Another session is not touched, and this one comes back with a fresh process.
    assert!(rig.runner.prompt(SESSION, "again", "/work").is_ok());
    assert_eq!(rig.spawner.spawned().len(), 2);
}

#[test]
fn a_failed_load_ends_waiting_sends_and_is_retried_by_a_new_send() {
    let responder: crate::tools::acp::fake::Responder = Arc::new(|m, p| {
        match m["method"].as_str() {
        Some("initialize") => ok(p, m, json!({"protocolVersion": 1})),
        Some("session/load") => p.push(&json!({"jsonrpc":"2.0","id":m["id"],"error":{"code":-1,"message":"no such conversation"}})),
        _ => {}
    }
    });
    let rig = rig_with(FakeSpawner::with_responder(responder), Some("/fake/agy"));
    let r = &rig.runner;
    r.prompt("x", "hello", "/work").unwrap();
    rig.events
        .wait("LoadFailed", |c| matches!(c, Change::LoadFailed(_)));
    rig.events.wait("the send ending", |c| {
        matches!(c, Change::TurnEnded(Some(_)))
    });
    assert_eq!(r.load_error("x").as_deref(), Some("no such conversation"));
    assert!(!r.is_working("x"), "the send that waited is over");
    assert!(rig.spawner.spawned()[0].killed(), "the process is closed");
    // Sending again retries at once, with a new process.
    r.prompt("x", "hello again", "/work").unwrap();
    assert_eq!(rig.spawner.spawned().len(), 2);
}

#[test]
fn a_new_session_gets_its_id_from_session_new() {
    let responder: crate::tools::acp::fake::Responder = Arc::new(|m, p| {
        match m["method"].as_str() {
            Some("initialize") => ok(p, m, json!({"protocolVersion": 1})),
            Some("session/new") => ok(
                p,
                m,
                json!({"sessionId": "made-1", "configOptions": [
            {"id": "model", "name": "Model", "type": "select", "currentValue": "m1",
             "options": [{"value": "m1", "name": "Model One"}, {"value": "m2", "name": "Model Two"}]}]}),
            ),
            _ => {}
        }
    });
    let rig = rig_with(FakeSpawner::with_responder(responder), Some("/fake/agy"));
    let id = rig.runner.start("/work", Duration::from_secs(10)).unwrap();
    assert_eq!(id, "made-1");
    assert!(rig.runner.is_open("made-1"));
    assert_eq!(rig.runner.config("made-1")[0].current_name(), "Model One");
    let new = &rig.spawner.spawned()[0].requests("session/new")[0];
    assert_eq!(new["params"], json!({"cwd": "/work", "mcpServers": []}));
    // It takes a prompt without another process.
    rig.runner.prompt("made-1", "hi", "/work").unwrap();
    assert_eq!(rig.spawner.spawned().len(), 1);
}

#[test]
fn a_new_session_that_never_answers_gives_up_and_leaves_nothing_behind() {
    let rig = rig_with(FakeSpawner::new(), Some("/fake/agy"));
    let err = rig
        .runner
        .start("/work", Duration::from_millis(150))
        .unwrap_err();
    assert!(matches!(err, RunError::Start(_)));
    assert!(rig.spawner.spawned()[0].killed());
    let refused = rig_with(FakeSpawner::new(), Some("/fake/agy"));
    refused.spawner.refuse_with("no such file");
    assert_eq!(
        refused.runner.start("/work", Duration::from_secs(1)),
        Err(RunError::Start(
            "Couldn't start Antigravity: no such file".into()
        ))
    );
    let none = rig_with(FakeSpawner::new(), None);
    assert_eq!(
        none.runner.start("/work", Duration::from_secs(1)),
        Err(RunError::NoBinary)
    );
    assert_eq!(none.runner.prompt("x", "t", "/w"), Err(RunError::NoBinary));
    assert!(none.spawner.calls().is_empty());
}

#[test]
fn options_are_checked_against_the_sessions_own_list_and_applied_from_the_reply() {
    let rig = rig();
    let process = loaded(&rig);
    let r = &rig.runner;
    let options = r.config(SESSION);
    let first = &options[0];
    let other = first
        .values
        .iter()
        .find(|v| v.value != first.current)
        .expect("a second value");
    assert!(matches!(
        r.set_option(SESSION, "nope", "x"),
        Err(RunError::Refused(_))
    ));
    assert!(matches!(
        r.set_option(SESSION, &first.id, "not-a-value"),
        Err(RunError::Refused(_))
    ));
    assert!(matches!(
        r.set_option("other", &first.id, &other.value),
        Err(RunError::Refused(_))
    ));
    r.set_option(SESSION, &first.id, &other.value).unwrap();
    let sent = process.requests("session/set_config_option");
    assert_eq!(
        sent[0]["params"],
        json!({"sessionId": SESSION, "configId": first.id, "value": other.value})
    );
    ok(&process, &sent[0], json!({}));
    rig.events.wait("Options", |c| *c == Change::Options);
    assert_eq!(r.config(SESSION)[0].current, other.value);
    // A refusal from the agent is reported, and the value stays.
    let before = r.config(SESSION);
    r.set_option(SESSION, &first.id, &first.current).unwrap();
    let again = process.requests("session/set_config_option");
    process.push(&json!({"jsonrpc":"2.0","id":again[1]["id"],"error":{"code":-1,"message":"model unavailable"}}));
    rig.events.wait("OptionFailed", |c| {
        *c == Change::OptionFailed("model unavailable".into())
    });
    assert_eq!(r.config(SESSION), before);
}

#[test]
fn an_idle_process_is_closed_after_ten_minutes_but_not_while_working_or_asking() {
    let rig = rig();
    let process = loaded(&rig);
    let r = &rig.runner;
    rig.clock.fetch_add(599, Ordering::SeqCst);
    r.reap();
    assert!(r.is_open(SESSION), "not yet");
    // Looking at it (counted as use) starts the ten minutes again; peeking does not.
    r.conversation(SESSION);
    rig.clock.fetch_add(300, Ordering::SeqCst);
    r.peek_conversation(SESSION);
    rig.clock.fetch_add(301, Ordering::SeqCst);
    r.reap();
    assert!(!r.is_open(SESSION));
    assert!(process.killed());
    rig.events.wait("Closed", |c| *c == Change::Closed);

    // A session with a permission waiting is kept however long it sits.
    r.prompt(SESSION, "edit", "/work").unwrap();
    rig.events.wait("a permission", |c| {
        matches!(c, Change::PermissionPending(_))
    });
    rig.clock.fetch_add(10_000, Ordering::SeqCst);
    r.reap();
    assert!(r.is_open(SESSION) && r.is_working(SESSION));
}

#[test]
fn a_load_that_never_answers_fails_after_the_load_time() {
    let rig = rig_with(FakeSpawner::new(), Some("/fake/agy"));
    rig.runner.open("hung", "/work", false).unwrap();
    assert!(rig.runner.is_loading("hung"));
    rig.clock.fetch_add(30, Ordering::SeqCst);
    rig.runner.reap();
    assert!(rig.runner.is_loading("hung"));
    rig.clock.fetch_add(40, Ordering::SeqCst);
    rig.runner.reap();
    rig.events
        .wait("LoadFailed", |c| matches!(c, Change::LoadFailed(_)));
    assert!(!rig.runner.is_loading("hung"));
    assert!(rig.spawner.spawned()[0].killed());
}

#[test]
fn a_second_permission_cancels_the_first_instead_of_leaving_it_hanging() {
    let rig = rig();
    let process = loaded(&rig);
    let ask = |id: &str| {
        process.push(&json!({"jsonrpc":"2.0","method":"session/request_permission","id":id,
            "params":{"toolCall":{"toolCallId":"c","name":"run_command","kind":"execute","title":"ls"},
                      "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}}));
    };
    ask("a");
    rig.events.wait(
        "first",
        |c| matches!(c, Change::PermissionPending(p) if p.rpc_text() == "a"),
    );
    ask("b");
    rig.events.wait(
        "second",
        |c| matches!(c, Change::PermissionPending(p) if p.rpc_text() == "b"),
    );
    let first = process
        .written()
        .into_iter()
        .find(|m| m["id"] == json!("a"))
        .unwrap();
    assert_eq!(first["result"]["outcome"]["outcome"], "cancelled");
    assert_eq!(rig.runner.permission(SESSION).unwrap().rpc_text(), "b");
    // An integer id is answered as an integer.
    process.push(&json!({"jsonrpc":"2.0","method":"session/request_permission","id":7,
        "params":{"toolCall":{"toolCallId":"d","name":"x","title":"x"},"options":[{"optionId":"deny","name":"Deny","kind":"reject_once"}]}}));
    rig.events.wait(
        "third",
        |c| matches!(c, Change::PermissionPending(p) if p.rpc_text() == "7"),
    );
    assert_eq!(
        rig.runner.answer(SESSION, "7", Some("deny")).unwrap(),
        "reject_once"
    );
    let third = process
        .written()
        .into_iter()
        .find(|m| m["id"] == json!(7))
        .unwrap();
    assert_eq!(third["result"]["outcome"]["optionId"], "deny");
}

fn ask(process: &FakeHandle, id: &str) {
    process.push(
        &json!({"jsonrpc":"2.0","method":"session/request_permission","id":id,
        "params":{"toolCall":{"toolCallId":"c","name":"run_command","kind":"execute","title":"ls"},
                  "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}}),
    );
}

/// Where in `events` the change `pred` picks first came, and that it came.
fn index_of(events: &[Change], pred: impl Fn(&Change) -> bool) -> usize {
    events
        .iter()
        .position(pred)
        .unwrap_or_else(|| panic!("no such change in {events:?}"))
}

fn cancelled_a(c: &Change) -> bool {
    matches!(c, Change::PermissionResolved { rpc_id, outcome } if rpc_id == "a" && outcome == "cancelled")
}

#[test]
fn closing_a_session_resolves_the_permission_waiting_in_it() {
    let rig = rig();
    let process = loaded(&rig);
    ask(&process, "a");
    rig.events
        .wait("pending", |c| matches!(c, Change::PermissionPending(_)));
    rig.runner.close(SESSION);
    rig.events.wait("Closed", |c| *c == Change::Closed);
    let events = rig.events.all();
    assert!(
        index_of(&events, cancelled_a) < index_of(&events, |c| *c == Change::Closed),
        "{events:?}"
    );
    assert_eq!(rig.events.count(cancelled_a), 1);
}

#[test]
fn a_process_that_dies_with_a_permission_waiting_resolves_it() {
    let rig = rig();
    let process = loaded(&rig);
    ask(&process, "a");
    rig.events
        .wait("pending", |c| matches!(c, Change::PermissionPending(_)));
    process.die(Some("agy crashed"));
    rig.events.wait("Closed", |c| *c == Change::Closed);
    let events = rig.events.all();
    assert!(
        index_of(&events, cancelled_a) < index_of(&events, |c| *c == Change::Closed),
        "{events:?}"
    );
    assert_eq!(rig.events.count(cancelled_a), 1);
}

#[test]
fn only_so_many_processes_are_open_at_once() {
    let rig = rig();
    let r = &rig.runner;
    r.set_max_open(2);
    r.open("a", "/work", false).unwrap();
    r.open("b", "/work", false).unwrap();
    let refused = r.open("c", "/work", false).unwrap_err();
    assert!(
        matches!(&refused, RunError::Refused(why) if why.contains('2')),
        "{refused:?}"
    );
    assert_eq!(rig.spawner.spawned().len(), 2, "nothing was started for it");
    // A new session needs a process too.
    assert!(matches!(
        r.start("/work", Duration::from_secs(1)),
        Err(RunError::Refused(_))
    ));
    assert_eq!(rig.spawner.spawned().len(), 2);
    // An open session is not a new process.
    r.open("a", "/work", false).unwrap();
    // Closing one makes room.
    r.close("a");
    r.open("c", "/work", false).unwrap();
    assert_eq!(rig.spawner.spawned().len(), 3);
}

#[test]
fn closing_everything_leaves_no_process() {
    let rig = rig();
    let process = loaded(&rig);
    rig.runner.close_all();
    assert!(process.killed());
    assert!(!rig.runner.is_open(SESSION));
}
