//! Port of `ProtocolFixturesTests`: the real router and hub, on the canned backend, against every
//! file in `protocol/fixtures/`. What the Mac sends must equal its fixture (key order aside); what
//! only the phone sends must be accepted. The Swift test writes the fixtures; this one only reads.

use super::fixtures_data::*;
use super::test_support::{is_ok, RecordingIo, Rig, NOW_MS};
use super::wire::{self, BridgeFailure};
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
    let rig = Rig::new();
    canned(&rig);
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

    /// Compares what the Mac sent with its fixture, after `scrub` replaced the random values.
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

    /// Sends a request fixture, expects `ok`, and compares the reply with `response`.
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

    // Two text frames for one request: the binary announcement, then `{sent: true}`.
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
    let models: Vec<Option<String>> = phone
        .rig
        .backend
        .with(|s| s.new_sessions.iter().map(|n| n.1.clone()).collect());
    assert_eq!(models, [Some("opus".to_string()), None]);

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
    // `lastMessageId` is the second message, so the next tick pushes the ones after it.
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

    // The permission is answered elsewhere: a `sessions` update and a resolved `prompt`.
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

    // A new permission appears on a session.
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
    // busy: the second interrupt inside the gate's three seconds.
    run.round_trip(&phone, "SessionSendNowRequest", json!({}), None);
    let busy = run.request("SessionSendNowRequest", json!({}));
    let busy = last(run.send(&phone, &busy));
    run.capture("ErrorResponse.busy", &busy, |_| {});
    let no_id = last(run.send(&phone, "not json"));
    run.capture("ErrorResponse.no-id", &no_id, |_| {});
    // The two codes only the live backend raises, through the same encoder.
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

/// The three binary frames after a JSON answer, as `protocol/README.md` lays them out.
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
