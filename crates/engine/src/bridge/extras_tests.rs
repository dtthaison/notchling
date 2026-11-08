//! The phone extras through the router and the live backend: `viz.get`, `chat.image`, `web.snapshot`
//! and pictures in `chat.get`, all over a real transcript on disk and fakes for what draws.

use super::extras::Extras;
use super::images::ImageCrate;
use super::live::{ActingOff, LiveBackend, Sources};
use super::model::*;
use super::router::{Router, RouterParts};
use super::snapshot::{SnapshotRequest, Snapshotter, WebSnapshots};
use super::stream::UnsupportedStreamer;
use super::test_support::*;
use super::wire::BridgeFailure;
use super::Connection;
use crate::sessions::testutil::TempDir;
use crate::viz::engine::GitSnapshot;
use crate::viz::VizEngine;
use base64::Engine;
use serde_json::{json, Value};
use std::io::Cursor;
use std::sync::{Arc, Mutex};

struct OneSession(Session);

impl Sources for OneSession {
    fn claude_sessions(&self) -> Vec<Session> {
        vec![self.0.clone()]
    }
    fn now_ms(&self) -> f64 {
        NOW_MS
    }
}

#[derive(Default)]
struct Shots {
    taken: Mutex<Vec<(String, u32, u32)>>,
}

struct FakeShots(Arc<Shots>);

impl Snapshotter for FakeShots {
    fn capture(&self, request: &SnapshotRequest) -> Result<Vec<u8>, BridgeFailure> {
        let (w, h) = request.view_size();
        self.0
            .taken
            .lock()
            .expect("taken")
            .push((request.url.clone(), w, h));
        Ok(JPEG.to_vec())
    }
}

struct World {
    router: Router,
    connection: Arc<Connection>,
    io: Arc<RecordingIo>,
    /// Kept alive for the folder; the file-marker tests (unix paths) also write into it.
    #[cfg_attr(not(unix), allow(dead_code))]
    dir: TempDir,
    shots: Arc<Shots>,
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let picture = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
    let mut out = Cursor::new(Vec::new());
    picture
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

fn user_line_with_picture(png: &[u8]) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    format!(
        r#"{{"type":"user","uuid":"u1","timestamp":"2026-11-03T08:00:01.000Z","message":{{"role":"user","content":[{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{data}"}}}},{{"type":"text","text":"look"}}]}}}}"#
    )
}

fn assistant_line(text: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-11-03T08:00:05.000Z","message":{{"id":"m1","role":"assistant","content":[{{"type":"text","text":{}}}]}}}}"#,
        serde_json::to_string(text).expect("text")
    )
}

/// A router over the live backend with one session whose transcript is `lines`.
fn world(lines: &[String]) -> World {
    let dir = TempDir::new("extras");
    std::fs::create_dir_all(dir.path("home/app")).expect("home");
    let transcript = dir.path("transcript.jsonl");
    std::fs::write(&transcript, lines.join("\n") + "\n").expect("transcript");
    let mut session = session("s1", Control::Job("job".into()), SessionState::Running);
    session.cwd = dir.path("home/app").to_string_lossy().into_owned();
    session.transcript_path = Some(transcript.to_string_lossy().into_owned());

    let shots = Arc::new(Shots::default());
    let home = dir.path("home");
    let extras = Extras {
        viz: VizEngine::new(
            Box::new(|| NOW_MS / 1000.0),
            Box::new(|_| GitSnapshot::default()),
        )
        .with_home(home.clone())
        .with_listening(Box::new(|port| port == 5173)),
        snapshots: WebSnapshots::new(Box::new(FakeShots(shots.clone())))
            .with_reachable(Box::new(|_| true)),
        streamer: Box::new(UnsupportedStreamer),
        imaging: Box::new(ImageCrate),
        home: home.clone(),
    };
    let backend: Arc<dyn BridgeBackend> =
        Arc::new(LiveBackend::new(OneSession(session), ActingOff).with_extras(extras));
    let devices = Arc::new(MemoryDevices::default());
    let hub = Arc::new(super::hub::Hub::new());
    let router = Router::new(RouterParts {
        backend,
        devices: devices.clone(),
        codes: Arc::new(MemoryCodes::default()),
        limiter: Arc::new(MemoryLimiter::default()),
        audit: Arc::new(MemoryAudit::default()),
        hub: Arc::clone(&hub),
        home,
        images_dir: dir.path("images"),
        now_ms: Arc::new(|| NOW_MS),
    });
    let (_, token) =
        super::backend::Devices::register(devices.as_ref(), "phone", "ios").expect("register");
    let io = Arc::new(RecordingIo::default());
    let connection = hub.connect("192.168.1.20", false, io.clone());
    router.handle(
        &connection,
        &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
    );
    World {
        router,
        connection,
        io,
        dir,
        shots,
    }
}

use super::backend::BridgeBackend;

impl World {
    fn ask(&self, request: Value) -> Value {
        self.router.handle(&self.connection, &request.to_string());
        self.io.last_json()
    }

    /// The ids of the pictures `chat.get` lists.
    fn picture_ids(&self) -> Vec<String> {
        let reply = self.ask(json!({"id":2,"op":"chat.get","sessionId":"s1"}));
        assert!(is_ok(&reply), "{reply}");
        data(&reply)["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .flat_map(|m| m["images"].as_array().cloned().unwrap_or_default())
            .map(|i| i["id"].as_str().expect("id").to_string())
            .collect()
    }
}

// MARK: Visualize

#[test]
fn viz_get_answers_the_three_specs_and_the_suggested_tab() {
    let world = world(&[assistant_line(
        "see http://localhost:5173/app\n```mermaid\ngraph TD; A-->B\n```",
    )]);
    let reply = world.ask(json!({"id":3,"op":"viz.get","sessionId":"s1"}));
    assert!(is_ok(&reply), "{reply}");
    let answer = data(&reply);
    assert_eq!(answer["flow"]["kind"], "flow");
    assert_eq!(answer["diagram"]["kind"], "diagram");
    assert_eq!(answer["chart"]["kind"], "chart");
    assert!(answer.get("entity").is_none(), "entity only when asked");
    assert_eq!(answer["webURL"], "http://localhost:5173/app");
    assert!(answer["suggested"].is_string());
}

#[test]
fn viz_get_adds_the_entity_spec_when_asked_and_says_when_it_cannot() {
    let world = world(&[assistant_line("hello")]);
    let reply = world.ask(json!({"id":3,"op":"viz.get","sessionId":"s1","entity":true}));
    assert!(is_ok(&reply), "{reply}");
    assert_eq!(data(&reply)["entity"]["kind"], "entity");
}

#[test]
fn viz_get_for_an_unknown_session_is_not_found() {
    let world = world(&[assistant_line("hello")]);
    let reply = world.ask(json!({"id":3,"op":"viz.get","sessionId":"nope"}));
    assert_eq!(error_code(&reply), Some("not_found"));
}

// MARK: Chat pictures

#[test]
fn chat_get_describes_a_picture_and_never_carries_its_bytes() {
    let world = world(&[user_line_with_picture(&png_bytes(40, 20))]);
    let reply = world.ask(json!({"id":2,"op":"chat.get","sessionId":"s1"}));
    let text = world.io.texts().last().cloned().expect("reply");
    assert!(
        text.len() < 5_000,
        "no base64 in the answer: {} bytes",
        text.len()
    );
    let images = data(&reply)["messages"][0]["images"].clone();
    assert_eq!(images.as_array().map(Vec::len), Some(1), "{reply}");
    assert_eq!(images[0]["mediaType"], "image/png");
    assert_eq!(images[0]["width"], 40);
    assert_eq!(images[0]["height"], 20);
    assert_eq!(images[0]["origin"], "user");
}

#[test]
fn chat_image_answers_a_jpeg_frame_for_a_picture_the_chat_listed() {
    let world = world(&[user_line_with_picture(&png_bytes(40, 20))]);
    let ids = world.picture_ids();
    assert_eq!(ids.len(), 1);
    let reply = world
        .ask(json!({"id":7,"op":"chat.image","sessionId":"s1","imageId":ids[0],"maxWidth":64}));
    assert!(is_ok(&reply), "{reply}");
    let announce = world.io.json(world.io.texts().len() - 2);
    assert_eq!(data(&announce)["binary"], true);
    assert_eq!(data(&announce)["imageId"], json!(ids[0]));
    let frame = world.io.binaries().pop().expect("frame");
    assert_eq!(&frame[..4], b"IMG1");
    assert_eq!(&frame[4..8], &7u32.to_be_bytes(), "the request id");
    assert_eq!(&frame[8..10], &[0xFF, 0xD8], "a JPEG");
    let decoded = image::load_from_memory(&frame[8..]).expect("decodes");
    assert_eq!(
        (decoded.width(), decoded.height()),
        (40, 20),
        "never enlarged"
    );
}

#[test]
fn chat_image_shrinks_to_the_asked_width() {
    let world = world(&[user_line_with_picture(&png_bytes(400, 200))]);
    let ids = world.picture_ids();
    let reply = world
        .ask(json!({"id":7,"op":"chat.image","sessionId":"s1","imageId":ids[0],"maxWidth":100}));
    assert!(is_ok(&reply), "{reply}");
    let frame = world.io.binaries().pop().expect("frame");
    let decoded = image::load_from_memory(&frame[8..]).expect("decodes");
    assert_eq!(decoded.width(), 100);
}

#[test]
fn chat_image_names_only_pictures_the_chat_listed() {
    let world = world(&[user_line_with_picture(&png_bytes(40, 20))]);
    let unknown =
        world.ask(json!({"id":7,"op":"chat.image","sessionId":"s1","imageId":"abcdef0123456789"}));
    assert_eq!(error_code(&unknown), Some("not_found"));
    for bad in ["../../etc/passwd", "a/b", "x y", &"a".repeat(81)] {
        let reply = world.ask(json!({"id":8,"op":"chat.image","sessionId":"s1","imageId":bad}));
        assert_eq!(error_code(&reply), Some("invalid"), "{bad}");
    }
    assert!(world.io.binaries().is_empty());
}

#[test]
fn chat_image_for_an_unknown_or_foreign_session_is_not_found() {
    let world = world(&[user_line_with_picture(&png_bytes(40, 20))]);
    let ids = world.picture_ids();
    for session in ["nope", "codex:abc", "agy:abc"] {
        let reply =
            world.ask(json!({"id":7,"op":"chat.image","sessionId":session,"imageId":ids[0]}));
        assert_eq!(error_code(&reply), Some("not_found"), "{session}");
    }
}

#[cfg(unix)]
#[test]
fn a_file_marker_outside_home_and_tmp_is_not_served() {
    let outside = TempDir::new("outside-secret");
    // The temp folder may itself sit under /tmp: only assert when it does not.
    let under_tmp =
        outside.root().starts_with("/tmp") || outside.root().starts_with("/private/tmp");
    if under_tmp {
        return;
    }
    let file = outside.path("secret.png");
    std::fs::write(&file, png_bytes(8, 8)).expect("file");
    let line = assistant_line(&format!("[Image: source: {}]", file.display()));
    let world = world(&[line]);
    assert!(
        world.picture_ids().is_empty(),
        "a marker outside home and /tmp lists nothing"
    );
}

#[cfg(unix)]
#[test]
fn a_file_marker_in_home_is_served_as_a_jpeg() {
    // The world's home exists only once it is built, so write the picture and then the transcript.
    let probe = world(&[assistant_line("x")]);
    let file = probe.dir.path("home/app/shot.png");
    std::fs::write(&file, png_bytes(30, 30)).expect("file");
    let line = assistant_line(&format!("saved [Image: source: {}]", file.display()));
    std::fs::write(probe.dir.path("transcript.jsonl"), line + "\n").expect("transcript");
    let ids = probe.picture_ids();
    assert_eq!(ids.len(), 1);
    let reply = probe.ask(json!({"id":7,"op":"chat.image","sessionId":"s1","imageId":ids[0]}));
    assert!(is_ok(&reply), "{reply}");
    assert_eq!(
        &probe.io.binaries().pop().expect("frame")[8..10],
        &[0xFF, 0xD8]
    );
}

// MARK: Web snapshots

#[test]
fn web_snapshot_with_a_url_shoots_it_at_a_phone_shaped_size() {
    let world = world(&[assistant_line("hello")]);
    let reply = world.ask(json!({"id":4,"op":"web.snapshot","sessionId":"s1","url":"http://localhost:3000/x","width":300}));
    assert!(is_ok(&reply), "{reply}");
    let frame = world.io.binaries().pop().expect("frame");
    assert_eq!(&frame[..4], b"WEB1");
    let taken = world.shots.taken.lock().expect("taken").clone();
    assert_eq!(taken, [("http://localhost:3000/x".to_string(), 300, 540)]);
}

#[test]
fn web_snapshot_without_a_url_uses_the_page_the_chat_named() {
    let world = world(&[assistant_line("running at http://localhost:5173/")]);
    let reply = world.ask(json!({"id":4,"op":"web.snapshot","sessionId":"s1"}));
    assert!(is_ok(&reply), "{reply}");
    let taken = world.shots.taken.lock().expect("taken").clone();
    assert_eq!(taken.len(), 1);
    assert!(taken[0].0.starts_with("http://localhost:5173"), "{taken:?}");
}

#[test]
fn web_snapshot_refuses_a_page_that_is_not_local() {
    let world = world(&[assistant_line("hello")]);
    let reply =
        world.ask(json!({"id":4,"op":"web.snapshot","sessionId":"s1","url":"http://example.com/"}));
    assert_eq!(error_code(&reply), Some("invalid"));
    assert!(world.shots.taken.lock().expect("taken").is_empty());
}

#[test]
fn web_snapshot_without_any_page_is_not_found() {
    let world = world(&[assistant_line("hello")]);
    let reply = world.ask(json!({"id":4,"op":"web.snapshot","sessionId":"s1"}));
    assert_eq!(error_code(&reply), Some("not_found"));
}

// MARK: Screen stream

#[test]
fn subscribing_to_the_phone_stream_here_says_it_is_unsupported() {
    let world = world(&[assistant_line("hello")]);
    let reply = world.ask(json!({"id":5,"op":"subscribe","phone":{"fps":5,"width":300}}));
    assert!(!is_ok(&reply), "{reply}");
    assert_eq!(error_code(&reply), Some("invalid"));
}
