use super::*;
use crate::acting_ops_tests::FakeRoom;
use notchling_engine::tools::acp::fake::FakeSpawner;
use notchling_engine::tools::agy_sidecar::jobs::Patience;
use std::sync::atomic::AtomicU32;

const OURS: &str = "79ba6320-1b32-46fe-838d-9bb614eb992d";
const TEST: &str = "457369a9-35a1-40f8-92b9-26b6398b78a1";
const NATIVE: &str = "bf68ea82-bc31-4c24-b8bf-8aa150aa05d4";
const CLI: &str = "9ddb7ec8-ff66-4511-b25a-0ad5a8ed8c8e";
const FRESH: &str = "5ce2bd47-0c1e-4e6b-8f43-6aa0e2f1d3aa";

struct World {
    base: PathBuf,
    ops: Arc<AgyOps>,
    room: Arc<FakeRoom>,
    spawner: Arc<FakeSpawner>,
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn world(tag: &str) -> World {
    static N: AtomicU32 = AtomicU32::new(0);
    let base = std::env::temp_dir().join(format!(
        "nl-agyapp-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&base);
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/fixtures/agy-app"),
        &base,
    );
    std::fs::create_dir_all(base.join("home")).unwrap();
    let room = Arc::new(FakeRoom::default());
    let spawner = Arc::new(FakeSpawner::replaying(""));
    let ops = AgyOps::new(
        Arc::new(Acting::in_memory(true)),
        room.clone(),
        spawner.clone(),
        AgySetup {
            root: base.join("antigravity-cli"),
            copy_dir: base.join("copy"),
            home: base.join("home"),
            binary: Some("/fake/agy".to_owned()),
            start_wait: Duration::from_secs(1),
            dispatch_wait: Duration::from_secs(1),
            timing: Timing::default(),
        },
    );
    World {
        base,
        ops: Arc::new(ops),
        room,
        spawner,
    }
}

fn choose_sidecar(w: &World) {
    let path = w.base.join("settings.json");
    notchling_engine::settings::Settings::open(&path)
        .unwrap()
        .set("antigravityChannel", json!("sidecar"))
        .unwrap();
    w.ops.set_settings(path);
}

fn ids(sessions: &Value) -> Vec<String> {
    let mut out: Vec<String> = sessions
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_owned())
        .collect();
    out.sort();
    out
}

fn items(reply: &Value) -> Vec<(String, String)> {
    reply["conversation"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| {
            let kind = &item["kind"];
            let text = kind["data"]["text"].as_str()?.to_owned();
            match kind["type"].as_str()? {
                "user" => Some(("you".to_owned(), text)),
                "assistant" => Some(("agy".to_owned(), text)),
                _ => None,
            }
        })
        .collect()
}

#[test]
fn the_sessions_list_has_the_cli_conversation_and_the_sidecars_app_ones_only() {
    let w = world("list");
    let sessions = w.ops.call(SESSIONS, &json!({})).unwrap();
    let mut want = vec![
        format!("agy:{TEST}"),
        format!("agy:{OURS}"),
        format!("agy:{CLI}"),
    ];
    want.sort();
    assert_eq!(ids(&sessions), want);
    assert!(!ids(&sessions).contains(&format!("agy:{NATIVE}")));
}

#[test]
fn an_app_conversation_reads_with_your_messages_and_the_agents_replies_and_never_starts_agy() {
    let w = world("read");
    let reply = w
        .ops
        .call(CONVERSATION, &json!({"id": format!("agy:{TEST}")}))
        .unwrap();
    assert_eq!(reply["open"], true);
    assert_eq!(reply["working"], false);
    assert_eq!(reply["loading"], false);
    let said = items(&reply);
    assert_eq!(said.len(), 8);
    assert_eq!(
        said[0],
        (
            "you".to_owned(),
            "Reply with the single word PONG and nothing else.".to_owned()
        )
    );
    assert_eq!(said[1], ("agy".to_owned(), "PONG".to_owned()));
    let state = w
        .ops
        .call(STATE, &json!({"id": format!("agy:{OURS}")}))
        .unwrap();
    assert_eq!(state["open"], true);
    assert_eq!(state["title"], "pong");
    assert_eq!(state["working"], false);
    let phone = w.ops.phone_read(OURS, false);
    assert_eq!(phone.conversation.items.len(), 3);
    assert!(w.spawner.calls().is_empty(), "no agy acp was started");
}

#[test]
fn opening_an_app_conversation_names_its_row_and_starts_nothing() {
    let w = world("open");
    let reply = w
        .ops
        .call(OPEN, &json!({"id": format!("agy:{OURS}")}))
        .unwrap();
    assert_eq!(reply["id"], format!("agy:{OURS}"));
    assert_eq!(reply["loading"], false);
    let changed = w.room.named(topics::SESSION_CHANGED);
    let last = changed.last().expect("a session.changed");
    assert_eq!(last["id"], format!("agy:{OURS}"));
    assert_eq!(last["change"], "loaded");
    assert_eq!(last["row"]["id"], format!("agy:{OURS}"));
    assert_eq!(last["row"]["name"], "pong");
    assert_eq!(last["row"]["group"], "completed");
    assert!(w.spawner.calls().is_empty());
}

#[test]
fn a_log_that_grows_is_announced_and_a_turn_in_progress_shows_as_working() {
    let w = world("grow");
    w.ops.app.set_gap(Duration::from_millis(20));
    w.ops
        .call(OPEN, &json!({"id": format!("agy:{OURS}")}))
        .unwrap();
    let before = w.room.named(topics::SESSION_CHANGED).len();
    let log = notchling_engine::tools::agy::app_store::log_path(
        &notchling_engine::tools::agy::app_store::root_beside(&w.base.join("antigravity-cli")),
        OURS,
    );
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str(concat!(
        r#"{"step_index":4,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2099-11-18T15:30:00Z","content":"x <SYSTEM_MESSAGE>\n[Message] timestamp=2099-11-18T15:30:00Z sender=system priority=MESSAGE_PRIORITY_HIGH content=Now count to three.\n</SYSTEM_MESSAGE>"}"#,
        "\n"
    ));
    std::fs::write(&log, text).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    let started = loop {
        let changed = w.room.named(topics::SESSION_CHANGED);
        if let Some(event) = changed[before..]
            .iter()
            .find(|e| e["change"] == "turn_started")
        {
            break event.clone();
        }
        assert!(Instant::now() < end, "no turn_started event");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(started["row"]["group"], "working");
    let reply = w
        .ops
        .call(CONVERSATION, &json!({"id": format!("agy:{OURS}")}))
        .unwrap();
    assert_eq!(reply["working"], true);
    assert_eq!(items(&reply).last().unwrap().1, "Now count to three.");
    assert_eq!(items(&reply).len(), 3);
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str(concat!(
        r#"{"step_index":5,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2099-11-18T15:30:05Z","input_tokens":10,"cache_read_tokens":0,"output_tokens":3,"content":"1, 2, 3"}"#,
        "\n"
    ));
    std::fs::write(&log, text).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let changed = w.room.named(topics::SESSION_CHANGED);
        if let Some(event) = changed.iter().find(|e| e["change"] == "turn_ended") {
            assert_eq!(event["row"]["group"], "completed");
            break;
        }
        assert!(Instant::now() < end, "no turn_ended event");
        std::thread::sleep(Duration::from_millis(10));
    }
    let reply = w
        .ops
        .call(CONVERSATION, &json!({"id": format!("agy:{OURS}")}))
        .unwrap();
    assert_eq!(reply["working"], false);
    assert_eq!(items(&reply).last().unwrap().1, "1, 2, 3");
}

#[test]
fn a_reply_to_an_app_conversation_is_refused_with_the_reason_unless_the_sidecar_is_chosen() {
    let w = world("reply");
    let refused = w
        .ops
        .call(SEND, &json!({"id": format!("agy:{OURS}"), "text": "hi"}))
        .unwrap_err();
    assert!(refused.message.contains("Antigravity app (full)"));
    assert!(w.ops.check_session(OURS).is_err());
    assert!(matches!(
        w.ops.deliver("job", OURS, "hi"),
        Err(Delivery::Failed(why)) if why.contains("Antigravity app (full)")
    ));
    choose_sidecar(&w);
    assert!(w.ops.check_session(OURS).is_ok());
    assert!(w.spawner.calls().is_empty());
}

#[test]
fn a_cli_conversation_is_still_found_through_the_cli_paths() {
    let w = world("cli");
    assert!(!w.ops.app.is_app(CLI));
    assert!(w.ops.folder_for(CLI).is_ok());
    assert!(w.ops.folder_for(OURS).is_ok());
    assert!(w.ops.folder_for(NATIVE).is_err());
    let refused = w
        .ops
        .call(OPEN, &json!({"id": format!("agy:{NATIVE}")}))
        .unwrap_err();
    assert!(refused.message.contains("no Antigravity session"));
}

#[test]
fn a_conversation_the_sidecar_just_started_is_a_row_at_once_before_the_app_lists_it() {
    let w = world("start");
    choose_sidecar(&w);
    let link = SidecarLink::with(
        Box::new(crate::acting_ops::now_ms),
        Patience {
            pickup: Duration::from_millis(600),
            run: Duration::from_millis(600),
        },
        Box::new(|_| true),
    );
    w.ops.attach_sidecar(link.clone());
    link.queue().seen(crate::acting_ops::now_ms());
    let taker = {
        let link = link.clone();
        std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(job) = link
                    .queue()
                    .take(Duration::from_millis(20))
                    .into_iter()
                    .next()
                {
                    link.queue().report(
                        &job.id,
                        notchling_engine::tools::agy_sidecar::jobs::Outcome::Delivered {
                            session: Some(FRESH.into()),
                        },
                    );
                    return;
                }
                assert!(Instant::now() < end, "no job came");
            }
        })
    };
    let folder = w.base.join("work");
    std::fs::create_dir_all(&folder).unwrap();
    let reply = w
        .ops
        .call(
            SEND,
            &json!({"draft_folder": folder.to_string_lossy(), "text": "Build the thing"}),
        )
        .unwrap();
    taker.join().unwrap();
    assert_eq!(reply["id"], format!("agy:{FRESH}"));
    let changed = w.room.named(topics::SESSION_CHANGED);
    let event = changed.last().expect("a session.changed for the new row");
    assert_eq!(event["id"], format!("agy:{FRESH}"));
    assert_eq!(event["row"]["id"], format!("agy:{FRESH}"));
    assert_eq!(event["row"]["name"], "Build the thing");
    assert_eq!(event["row"]["group"], "working");
    let sessions = w.ops.call(SESSIONS, &json!({})).unwrap();
    assert!(ids(&sessions).contains(&format!("agy:{FRESH}")));
    let read = w
        .ops
        .call(CONVERSATION, &json!({"id": format!("agy:{FRESH}")}))
        .unwrap();
    assert_eq!(read["open"], true);
    assert!(w.ops.app.is_app(FRESH));
    assert!(w.spawner.calls().is_empty());
}
