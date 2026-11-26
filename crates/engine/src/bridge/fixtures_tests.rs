use super::fixtures_data::*;
use super::relay::{HeldNoticeView, HeldView};
use super::test_support::{
    inbox_item, is_ok, scripted_cluster, Plan, RecordingIo, Rig, ScriptedRelay, NOW_MS,
};
use super::wire::{self, BridgeFailure};
use crate::inbox::{ActionId, AlertAction, Kind, Outcome, OutcomeState, Place, State, Waiter};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocol/fixtures")
}

struct Run {
    touched: BTreeSet<String>,
}

struct Phone {
    rig: Rig,
    connection: Arc<super::connection::Connection>,
    io: Arc<RecordingIo>,
}

fn canned(rig: &Rig) {
    rig.backend.with(|s| {
        s.sessions = sessions();
        s.chat = chat();
        s.commands = commands();
        s.prompts = [permission(), question()]
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect();
    });
}

fn connect() -> Phone {
    connect_with(None)
}

fn connect_with(relay: Option<Arc<ScriptedRelay>>) -> Phone {
    let rig = Rig::new();
    canned(&rig);
    if let Some(relay) = relay {
        rig.backend.with(|s| s.relay = Some(relay));
    }
    std::fs::create_dir_all(rig.home().join("src/app")).expect("project");
    let (_, token) = super::backend::Devices::register(rig.devices.as_ref(), "Son's iPhone", "ios")
        .expect("register");
    let (connection, io) = rig.connect("192.168.1.20");
    rig.send(
        &connection,
        &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
    );
    assert!(connection.is_authenticated());
    Phone {
        rig,
        connection,
        io,
    }
}

impl Run {
    fn load(&self, name: &str) -> Value {
        let path = fixtures_dir().join(format!("{name}.json"));
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        serde_json::from_str(&text).expect("fixture json")
    }

    fn request(&mut self, name: &str, patch: Value) -> String {
        self.touched.insert(name.to_string());
        let mut object = self.load(name);
        if let (Some(map), Value::Object(changes)) = (object.as_object_mut(), patch) {
            map.extend(changes);
        }
        object.to_string()
    }

    fn capture(&mut self, name: &str, text: &str, scrub: impl FnOnce(&mut Value)) {
        self.touched.insert(name.to_string());
        let mut sent: Value =
            serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}: {text}"));
        scrub(&mut sent);
        let saved = self.load(name);
        assert_eq!(
            saved, sent,
            "{name}.json differs from what the Mac sends:\n{text}"
        );
    }

    fn send(&self, phone: &Phone, text: &str) -> Vec<String> {
        let before = phone.io.texts().len();
        phone.rig.send(&phone.connection, text);
        phone.io.texts()[before..].to_vec()
    }

    fn round_trip_failure(&mut self, phone: &Phone, request: &str, patch: Value) -> String {
        let text = self.request(request, patch);
        self.send(phone, &text).last().expect("an answer").clone()
    }

    fn round_trip(
        &mut self,
        phone: &Phone,
        request: &str,
        patch: Value,
        response: Option<&str>,
    ) -> Vec<String> {
        let text = self.request(request, patch);
        let texts = self.send(phone, &text);
        let last = texts
            .last()
            .unwrap_or_else(|| panic!("{request}: no reply"))
            .clone();
        let reply: Value = serde_json::from_str(&last).expect("json");
        assert!(is_ok(&reply), "{request}: {last}");
        if let Some(response) = response {
            self.capture(response, &last, |_| {});
        }
        texts
    }
}

fn set_data(object: &mut Value, key: &str, value: Value) {
    if let Some(data) = object.get_mut("data").and_then(Value::as_object_mut) {
        data.insert(key.to_string(), value);
    }
}

#[test]
fn every_fixture_is_captured_or_accepted() {
    let mut run = Run {
        touched: BTreeSet::new(),
    };
    authentication(&mut run);
    queries(&mut run);
    actions(&mut run);
    uploads(&mut run);
    subscriptions(&mut run);
    inbox(&mut run);
    machines(&mut run);
    relay(&mut run);
    errors(&mut run);
    binary_frames();

    let names: BTreeSet<String> = std::fs::read_dir(fixtures_dir())
        .expect("protocol/fixtures")
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
                .map(String::from)
        })
        .collect();
    let untouched: Vec<_> = names.difference(&run.touched).collect();
    assert!(
        untouched.is_empty(),
        "fixtures nothing sends or compares: {untouched:?}"
    );
    let missing: Vec<_> = run.touched.difference(&names).collect();
    assert!(missing.is_empty(), "fixtures this test needs: {missing:?}");
}

fn authentication(run: &mut Run) {
    let rig = Rig::new();
    canned(&rig);
    let (_, token) = super::backend::Devices::register(rig.devices.as_ref(), "Son's iPhone", "ios")
        .expect("register");

    let (hello, hello_io) = rig.connect("192.168.1.20");
    rig.send(
        &hello,
        &run.request("HelloRequest", json!({ "token": token })),
    );
    assert!(hello.is_authenticated());
    run.capture("HelloResponse", &hello_io.texts()[0], |o| {
        set_data(
            o,
            "device",
            json!({"id": "6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90", "name": "Son's iPhone"}),
        );
    });

    let (rejected, rejected_io) = rig.connect("192.168.1.30");
    rig.send(&rejected, &run.request("HelloRequest", json!({})));
    run.capture(
        "ErrorResponse.unauthorized",
        &rejected_io.texts()[0],
        |_| {},
    );

    let (pairing, pairing_io) = rig.connect("192.168.1.31");
    let code = rig.codes.issue(NOW_MS);
    rig.send(
        &pairing,
        &run.request("PairRequest", json!({ "code": code })),
    );
    assert!(pairing.is_authenticated());
    run.capture("PairResponse", &pairing_io.texts()[0], |o| {
        set_data(o, "token", json!("tok-new-device-1111111111111111"));
        set_data(o, "deviceId", json!("9B3E7C14-2A60-4D85-8F1E-0C5D7A9B3E62"));
    });
}

fn queries(run: &mut Run) {
    let phone = connect();
    let none = || json!({});
    run.round_trip(&phone, "PingRequest", none(), Some("PingResponse"));
    run.round_trip(
        &phone,
        "PingRequest.string-id",
        none(),
        Some("PingResponse.string-id"),
    );
    run.round_trip(
        &phone,
        "SessionsListRequest",
        none(),
        Some("SessionsListResponse"),
    );
    phone.rig.backend.with(|s| s.models = models());
    run.round_trip(
        &phone,
        "SessionsListRequest",
        none(),
        Some("SessionsListResponse.models"),
    );
    phone.rig.backend.with(|s| s.models.clear());
    phone.rig.backend.with(|s| {
        if let Some(facts) = s.sessions[0].facts.as_mut() {
            facts.effort = None;
        }
    });
    run.round_trip(
        &phone,
        "SessionsListRequest",
        none(),
        Some("SessionsListResponse.no-effort"),
    );
    phone.rig.backend.with(|s| {
        if let Some(facts) = s.sessions[0].facts.as_mut() {
            facts.effort = Some("xhigh".into());
        }
    });
    phone.rig.backend.with(|s| {
        s.sessions[1].node = Some(super::model::SessionNode {
            id: "0123456789abcdef".into(),
            name: "mac-mini".into(),
            online: false,
        })
    });
    run.round_trip(
        &phone,
        "SessionsListRequest",
        none(),
        Some("SessionsListResponse.node"),
    );
    phone.rig.backend.with(|s| s.sessions[1].node = None);
    run.round_trip(&phone, "ChatGetRequest", none(), Some("ChatGetResponse"));
    run.round_trip(
        &phone,
        "ChatGetRequest.before",
        none(),
        Some("ChatGetResponse.before"),
    );
    run.round_trip(
        &phone,
        "ChatGetRequest.after-gone",
        none(),
        Some("ChatGetResponse.reset"),
    );
    run.round_trip(&phone, "ChatGetRequest.subagent", none(), None);
    phone.rig.backend.with(|s| s.chat = vec![compaction()]);
    run.round_trip(
        &phone,
        "ChatGetRequest",
        none(),
        Some("ChatGetResponse.compaction"),
    );
    run.round_trip(
        &phone,
        "CommandsListRequest",
        none(),
        Some("CommandsListResponse"),
    );
    run.round_trip(
        &phone,
        "ProjectsRecentRequest",
        none(),
        Some("ProjectsRecentResponse"),
    );
    run.round_trip(&phone, "VizGetRequest", none(), Some("VizGetResponse"));
    run.round_trip(
        &phone,
        "VizGetRequest.entity",
        none(),
        Some("VizGetResponse.entity"),
    );

    let snapshot = run.round_trip(&phone, "WebSnapshotRequest", none(), None);
    assert_eq!(snapshot.len(), 2);
    run.capture("WebSnapshotResponse", &snapshot[0], |_| {});
    run.capture("SentResponse", &snapshot[1], |_| {});
    let by_url = run.round_trip(&phone, "WebSnapshotRequest.url", none(), None);
    assert_eq!(by_url.len(), 2);
    let image = run.round_trip(&phone, "ChatImageRequest", none(), None);
    assert_eq!(image.len(), 2);
    run.capture("ChatImageResponse", &image[0], |_| {});
    assert_eq!(
        phone.io.binaries().len(),
        3,
        "a binary frame after each of the three"
    );
}

fn actions(run: &mut Run) {
    let phone = connect();
    let project = phone
        .rig
        .home()
        .join("src/app")
        .to_string_lossy()
        .into_owned();
    let none = || json!({});
    run.round_trip(
        &phone,
        "SessionMessageRequest",
        none(),
        Some("SessionMessageResponse"),
    );
    let sent: Vec<String> = phone
        .rig
        .backend
        .with(|s| s.sent.iter().map(|x| x.1.clone()).collect());
    assert_eq!(sent, ["Run the tests again"]);
    run.round_trip(
        &phone,
        "SessionSendNowRequest",
        none(),
        Some("SessionSendNowResponse"),
    );
    assert_eq!(
        phone.rig.backend.with(|s| s.interrupted.clone()),
        [SESSION_A]
    );

    run.round_trip(
        &phone,
        "PromptAnswerRequest",
        none(),
        Some("PromptAnswerResponse"),
    );
    for variant in ["deny", "choice", "label", "text"] {
        run.round_trip(
            &phone,
            &format!("PromptAnswerRequest.{variant}"),
            none(),
            None,
        );
    }
    let answered: Vec<String> = phone
        .rig
        .backend
        .with(|s| s.responses.iter().map(|r| r.0.clone()).collect());
    assert_eq!(
        answered,
        ["req-0001", "req-0001", "req-0002", "req-0002", "req-0002"]
    );

    run.round_trip(
        &phone,
        "SessionNewRequest",
        json!({ "cwd": project }),
        Some("SessionNewResponse"),
    );
    run.round_trip(
        &phone,
        "SessionNewRequest.no-model",
        json!({ "cwd": project }),
        None,
    );
    phone.rig.backend.with(|s| s.models = models());
    run.round_trip(
        &phone,
        "SessionNewRequest.model-id",
        json!({ "cwd": project }),
        None,
    );
    for keyed in ["SessionMessageRequest.key", "PromptAnswerRequest.key"] {
        run.round_trip(&phone, keyed, none(), None);
    }
    run.round_trip(&connect(), "SessionSendNowRequest.key", none(), None);
    run.round_trip(
        &phone,
        "SessionNewRequest.key",
        json!({ "cwd": project }),
        None,
    );
    let started: Vec<Option<String>> = phone
        .rig
        .backend
        .with(|s| s.new_sessions.iter().map(|n| n.1.clone()).collect());
    assert_eq!(
        started,
        [
            Some("opus".to_string()),
            None,
            Some("claude-fable-5-2".to_string()),
            Some("opus".to_string())
        ]
    );

    run.round_trip(
        &phone,
        "SessionTakeoverRequest",
        none(),
        Some("SessionTakeoverResponse"),
    );
    run.round_trip(
        &phone,
        "SessionTakeoverRequest.plain",
        none(),
        Some("SessionTakeoverResponse.plain"),
    );
    let flags: Vec<bool> = phone
        .rig
        .backend
        .with(|s| s.takeovers.iter().map(|t| t.1).collect());
    assert_eq!(flags, [true, false]);
}

fn uploads(run: &mut Run) {
    let phone = connect();
    let begin = run.request("SessionImageBeginRequest", json!({}));
    let begin = run.send(&phone, &begin);
    let reply: Value = serde_json::from_str(begin.last().expect("reply")).expect("json");
    let upload_id = reply["data"]["uploadId"]
        .as_str()
        .expect("uploadId")
        .to_string();
    run.capture(
        "SessionImageBeginResponse",
        begin.last().expect("reply"),
        |o| {
            set_data(o, "uploadId", json!("upload-example"));
        },
    );
    let patch = || json!({ "uploadId": upload_id });
    run.round_trip(
        &phone,
        "SessionImageChunkRequest",
        patch(),
        Some("SessionImageChunkResponse"),
    );
    run.round_trip(
        &phone,
        "SessionImageEndRequest",
        patch(),
        Some("SessionImageEndResponse"),
    );
    let last = phone
        .rig
        .backend
        .with(|s| s.sent.last().cloned())
        .expect("sent");
    assert!(
        last.1.starts_with("What is this? "),
        "the caption then the saved file's path: {}",
        last.1
    );
}

fn subscriptions(run: &mut Run) {
    let phone = connect();
    let subscribed = run.round_trip(
        &phone,
        "SubscribeRequest",
        json!({}),
        Some("SubscribeResponse"),
    );
    assert_eq!(
        subscribed.len(),
        2,
        "the first `sessions` snapshot, then the answer"
    );
    run.capture("SessionsEvent", &subscribed[0], |_| {});

    phone.rig.hub.tick(phone.rig.backend.as_ref());
    let first = phone.io.texts().len();
    let chat_text = phone.io.texts().last().cloned().expect("chat");
    assert!(chat_text.contains("\"event\":\"chat\""), "{chat_text}");
    run.capture("ChatEvent", &chat_text, |_| {});

    phone.rig.backend.with(|s| {
        s.sessions = sessions();
        s.sessions[0].pending = None;
    });
    phone.rig.hub.tick(phone.rig.backend.as_ref());
    let resolved = phone.io.texts()[first..]
        .iter()
        .find(|t| t.contains("\"resolved\":true"))
        .cloned()
        .expect("resolved");
    run.capture("PromptResolvedEvent", &resolved, |_| {});

    let mut fresh = permission();
    fresh.id = "req-0003".into();
    fresh.tool_name = "Write".into();
    fresh.summary = "Write /Users/example/src/app/notes.md".into();
    phone.rig.backend.with(|s| {
        s.sessions = sessions();
        s.sessions[0].pending = Some(fresh);
    });
    let before = phone.io.texts().len();
    phone.rig.hub.tick(phone.rig.backend.as_ref());
    let prompt = phone.io.texts()[before..]
        .iter()
        .find(|t| t.contains("\"event\":\"prompt\""))
        .cloned()
        .expect("prompt");
    run.capture("PromptEvent", &prompt, |_| {});

    assert_eq!(phone.rig.hub.notify_open(), 1);
    run.capture(
        "OpenEvent",
        &phone.io.texts().last().cloned().expect("open"),
        |_| {},
    );

    let unsubscribed = run.round_trip(
        &phone,
        "UnsubscribeRequest",
        json!({}),
        Some("UnsubscribeResponse"),
    );
    assert_eq!(unsubscribed.len(), 1);
    run.round_trip(&phone, "SubscribeRequest.minimal", json!({}), None);
    run.round_trip(&phone, "SubscribeRequest.phone", json!({}), None);
    let starts = phone.rig.backend.with(|s| s.phone_starts.clone());
    assert_eq!(
        starts,
        [super::model::PhoneOptions {
            device_window_id: Some(1234),
            fps: 10,
            width: 400
        }]
    );
}

fn inbox(run: &mut Run) {
    let phone = connect();
    phone
        .rig
        .backend
        .with(|s| s.inbox = vec![inbox_item("vault-approval-1", Kind::SecretApproval)]);
    run.round_trip(
        &phone,
        "InboxListRequest",
        json!({}),
        Some("InboxListResponse"),
    );
    run.round_trip(&phone, "SubscribeRequest.inbox", json!({}), None);
    phone.rig.hub.tick(phone.rig.backend.as_ref());
    let pushed = phone.io.texts().last().cloned().expect("inbox event");
    run.capture("InboxEvent", &pushed, |_| {});
    run.round_trip(
        &phone,
        "InboxAnswerRequest",
        json!({}),
        Some("InboxAnswerResponse"),
    );
    phone.rig.backend.with(|s| {
        s.inbox[0].answer = Some(ActionId::Deny);
    });
    let taken = run.round_trip(&phone, "InboxAnswerRequest", json!({}), None);
    run.capture(
        "InboxAnswerResponse.mac-first",
        taken.last().expect("reply"),
        |_| {},
    );
    phone.rig.backend.with(|s| s.inbox.clear());
    phone.rig.hub.tick(phone.rig.backend.as_ref());
    let removed = phone.io.texts().last().cloned().expect("removed event");
    run.capture("InboxRemovedEvent", &removed, |_| {});
    phone
        .rig
        .backend
        .with(|s| s.inbox = vec![inbox_item("approval-2", Kind::SecretApproval)]);
    let noted = run.round_trip_failure(&phone, "InboxAnswerRequest.note", json!({}));
    assert!(
        noted.contains("Notes from the phone aren't on yet."),
        "{noted}"
    );
    let mut judged = inbox_item("consent-9", Kind::Consent);
    judged.title = "Claude Code asks to change a setting".into();
    judged.detail = String::new();
    judged.created_ms = NOW_MS as i64 + 5;
    judged.expires_ms = None;
    judged.state = State::Answered;
    judged.answer = Some(ActionId::Allow);
    judged.answered_by = Some("mac".into());
    judged.closed_ms = Some(NOW_MS as i64 + 9);
    judged.seen = true;
    judged.blocking = Some(true);
    judged.waiting_since = Some(NOW_MS as i64 + 5);
    judged.waiter = Some(Waiter::Cli);
    judged.alert_actions = vec![AlertAction::Allow, AlertAction::Deny, AlertAction::Change];
    judged.note = Some("use the staging value".into());
    judged.outcome = Some(Outcome {
        state: OutcomeState::Failed,
        reason: Some("The setting could not be changed.".into()),
        at_ms: NOW_MS as i64 + 9,
    });
    judged.place = Some(Place {
        machine: Some("0123456789abcdef".into()),
        machine_name: Some("build".into()),
        session: Some("5b6d9a30-6f0e-4d0a-9b57-0c1f3a2e8d11".into()),
        session_name: Some("fix login".into()),
        branch: Some("main".into()),
    });
    phone.rig.backend.with(|s| s.inbox = vec![judged]);
    run.round_trip(
        &phone,
        "InboxListRequest",
        json!({}),
        Some("InboxListResponse.full"),
    );
}

fn machines(run: &mut Run) {
    let phone = connect();
    let ask = |run: &mut Run, request: &str, patch: Value, response: &str| {
        let texts = run.round_trip(&phone, request, patch, None);
        let reply = texts.last().expect("a reply").clone();
        run.capture(response, &reply, |_| {});
    };
    let none = || json!({});
    ask(
        run,
        "ClusterMachinesRequest",
        none(),
        "ClusterMachinesResponse.empty",
    );
    phone.rig.backend.with(|s| s.machines = cluster_machines());
    ask(
        run,
        "ClusterMachinesRequest",
        none(),
        "ClusterMachinesResponse",
    );
    let risk = "m5 can't be reached, so Notchling can't show that its work is safe on m1. Work m5 did after 14:02 stays on m5.";
    let relayed = json!({"relayed": true, "text": "m5 is reachable through m1; nothing moved"});
    let answers = [
        (
            "ClusterPromoteRequest",
            none(),
            json!({ "needsConfirm": risk }),
            "ClusterPromoteResponse.needs-confirm",
        ),
        (
            "ClusterPromoteRequest.confirmed",
            none(),
            json!({"step": "bump"}),
            "ClusterPromoteResponse",
        ),
        (
            "ClusterPromoteRequest",
            json!({"id": 54}),
            relayed,
            "ClusterPromoteResponse.relayed",
        ),
    ];
    for (request, patch, answer, response) in answers {
        phone
            .rig
            .backend
            .with(|s| s.promote_answer = Some(Ok(answer)));
        ask(run, request, patch, response);
    }
}

fn errors(run: &mut Run) {
    let phone = connect();
    let last = |texts: Vec<String>| texts.last().cloned().expect("reply");
    let unknown = last(run.send(&phone, r#"{"id":9,"op":"nope"}"#));
    run.capture("ErrorResponse.invalid", &unknown, |_| {});
    let missing = run.request(
        "ChatGetRequest",
        json!({"sessionId": "00000000-0000-4000-8000-000000000000"}),
    );
    let missing = last(run.send(&phone, &missing));
    run.capture("ErrorResponse.not_found", &missing, |_| {});
    let view_only = run.request("SessionMessageRequest", json!({ "sessionId": SESSION_B }));
    let view_only = last(run.send(&phone, &view_only));
    run.capture("ErrorResponse.view_only", &view_only, |_| {});
    run.round_trip(&phone, "SessionSendNowRequest", json!({}), None);
    let busy = run.request("SessionSendNowRequest", json!({}));
    let busy = last(run.send(&phone, &busy));
    run.capture("ErrorResponse.busy", &busy, |_| {});
    let no_id = last(run.send(&phone, "not json"));
    run.capture("ErrorResponse.no-id", &no_id, |_| {});
    let nine = json!(9);
    let unreachable = wire::error(
        Some(&nine),
        &BridgeFailure::not_reachable("localhost:3000 is not running on the Mac."),
    );
    run.capture("ErrorResponse.not_reachable", &unreachable, |_| {});
    let internal = wire::error(
        Some(&nine),
        &BridgeFailure::internal("Internal error on the Mac."),
    );
    run.capture("ErrorResponse.internal", &internal, |_| {});
}

fn binary_frames() {
    let jpeg = [0xFF, 0xD8, 0xFF, 0xD9];
    let tail = [1, 2, 3, 4, 0xFF, 0xD8, 0xFF, 0xD9];
    assert_eq!(
        wire::frame::image(0x0102_0304, &jpeg),
        [b"IMG1".as_slice(), &tail].concat()
    );
    assert_eq!(
        wire::frame::snapshot(0x0102_0304, &jpeg),
        [b"WEB1".as_slice(), &tail].concat()
    );
    assert_eq!(
        wire::frame::phone(7, &jpeg),
        [b"PHN1".as_slice(), &[0, 0, 0, 7], &jpeg].concat()
    );
}

fn held_entries() -> Vec<HeldView> {
    vec![
        HeldView {
            id: "held-1".into(),
            key: Some("7f9c2d44-5b1e-4a3a-9d0e-2c6b8e1f4a77".into()),
            device: String::new(),
            op: "session.message".into(),
            session_id: Some("0123456789abcdef/job-7".into()),
            summary: "Run the tests again".into(),
            at_ms: 1_793_541_465_000,
            waiting: false,
        },
        HeldView {
            id: "held-2".into(),
            key: None,
            device: String::new(),
            op: "session.new".into(),
            session_id: None,
            summary: "New session in /Users/example/src/app".into(),
            at_ms: 1_793_537_865_000,
            waiting: true,
        },
    ]
}

fn relay(run: &mut Run) {
    let none = || json!({});

    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    let phone = connect_with(Some(relay.clone()));
    let hello = phone.io.texts()[0].clone();
    run.request("HelloRequest", none());
    run.capture("HelloResponse.cluster", &hello, |o| {
        set_data(
            o,
            "device",
            json!({"id": "6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90", "name": "Son's iPhone"}),
        );
    });

    relay.plan(Plan::Relayed);
    let sent = run.round_trip(&phone, "SessionMessageRequest", json!({"id": 9}), None);
    run.capture("SentResponse.relayed", &sent[0], |_| {});
    let project = phone
        .rig
        .home()
        .join("src/app")
        .to_string_lossy()
        .into_owned();
    let started = run.round_trip(&phone, "SessionNewRequest", json!({"cwd": project}), None);
    run.capture("SessionNewResponse.relayed", &started[0], |_| {});
    let answered = run.round_trip(&phone, "PromptAnswerRequest", none(), None);
    run.capture("PromptAnswerResponse.relayed", &answered[0], |_| {});

    relay.plan(Plan::Fail(BridgeFailure::primary_unreachable(
        "m5",
        "m5 can't be reached, and no copy of that chat is on this Mac.",
    )));
    let refused = run.round_trip_failure(&phone, "SessionMessageRequest", json!({"id": 46}));
    run.capture("ErrorResponse.primary_unreachable", &refused, |_| {});

    *relay.stale.lock().expect("stale") = Some(1_793_541_000_000);
    run.round_trip(
        &phone,
        "SessionsListRequest",
        none(),
        Some("SessionsListResponse.stale"),
    );
    run.round_trip(
        &phone,
        "ChatGetRequest",
        none(),
        Some("ChatGetResponse.stale"),
    );
    *relay.stale.lock().expect("stale") = None;

    let empty = connect_with(Some(ScriptedRelay::new(None)));
    run.round_trip(
        &empty,
        "HeldListRequest",
        none(),
        Some("HeldListResponse.empty"),
    );

    let relay = ScriptedRelay::new(None);
    *relay.entries.lock().expect("entries") = held_entries();
    *relay.notices.lock().expect("notices") = vec![HeldNoticeView {
        id: "held-0".into(),
        device: String::new(),
        reason: "m5 closed that question before your answer arrived".into(),
        at_ms: 1_793_541_465_500,
    }];
    let phone = connect_with(Some(relay.clone()));
    let subscribed = run.round_trip(&phone, "SubscribeRequest.held", none(), None);
    assert_eq!(subscribed.len(), 2, "the held state, then the answer");
    run.capture("HeldChangedEvent", &subscribed[0], |_| {});
    run.round_trip(&phone, "HeldListRequest", none(), Some("HeldListResponse"));
    run.round_trip(
        &phone,
        "HeldConfirmRequest",
        none(),
        Some("HeldConfirmResponse"),
    );
    run.round_trip(
        &phone,
        "HeldCancelRequest",
        none(),
        Some("HeldCancelResponse"),
    );
    relay
        .delivered
        .lock()
        .expect("delivered")
        .push("held-1".into());
    run.round_trip(
        &phone,
        "HeldCancelRequest",
        none(),
        Some("HeldCancelResponse.delivered"),
    );
    relay
        .inflight_keys
        .lock()
        .expect("inflight")
        .push("7f9c2d44-5b1e-4a3a-9d0e-2c6b8e1f4a77".into());
    run.round_trip(
        &phone,
        "HeldCancelRequest.key",
        none(),
        Some("HeldCancelResponse.inFlight"),
    );

    relay.plan(Plan::Hold);
    relay.entries.lock().expect("entries").clear();
    let held = run.round_trip(&phone, "SessionMessageRequest", json!({"id": 44}), None);
    run.capture("HeldResponse", held.last().expect("answer"), |_| {});
}
