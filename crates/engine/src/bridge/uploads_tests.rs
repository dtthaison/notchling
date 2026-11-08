//! Photos from the phone through the router (Swift `PhoneUploadTests`) and the hostile cases the
//! M4 review names: a name that climbs out of the folder, a huge or never-finished upload, a photo
//! typed into a session while acting is off.

use super::backend::BridgeBackend;
use super::live::{Acting, ActingOff, LiveBackend, Sources, ACTING_OFF};
use super::model::*;
use super::router::{Router, RouterParts};
use super::test_support::*;
use super::uploads::{ImageStore, UploadError, Uploads, EXPIRES_AFTER_MS, MAX_BYTES};
use super::wire::BridgeFailure;
use crate::sessions::testutil::TempDir;
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn jpeg() -> Vec<u8> {
    let mut data = vec![0xFF, 0xD8, 0xFF, 0xE0];
    data.extend(std::iter::repeat_n(7, 300_000));
    data
}

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Begins an upload and sends it in `chunk`-sized pieces; the last answer is `end`'s.
fn upload(
    rig: &Rig,
    connection: &Arc<super::Connection>,
    io: &RecordingIo,
    session: &str,
    caption: &str,
    data: &[u8],
) -> Value {
    rig.send(
        connection,
        &json!({"id":10,"op":"session.image.begin","sessionId":session,"bytes":data.len(),"caption":caption}).to_string(),
    );
    let begin = io.last_json();
    assert!(is_ok(&begin), "{begin}");
    let upload_id = data_of(&begin)["uploadId"]
        .as_str()
        .expect("id")
        .to_string();
    for (n, piece) in data.chunks(96 * 1024).enumerate() {
        rig.send(
            connection,
            &json!({"id":20+n,"op":"session.image.chunk","uploadId":upload_id,"seq":n,"data":b64(piece)}).to_string(),
        );
        assert!(is_ok(&io.last_json()), "chunk {n}");
    }
    rig.send(
        connection,
        &json!({"id":99,"op":"session.image.end","uploadId":upload_id}).to_string(),
    );
    io.last_json()
}

fn data_of(reply: &Value) -> Value {
    super::test_support::data(reply)
}

fn saved_files(rig: &Rig) -> Vec<PathBuf> {
    std::fs::read_dir(rig.dir.path("images"))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

#[test]
fn a_photo_arrives_in_pieces_is_saved_privately_and_its_path_goes_to_the_session() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session(
            "p",
            Control::Job("job".into()),
            SessionState::Running,
        )]
    });
    let (connection, io, _) = rig.login("phone");
    let data = jpeg();
    let reply = upload(&rig, &connection, &io, "p", "look", &data);
    assert!(is_ok(&reply), "{reply}");
    let (session_id, text) = rig.backend.with(|s| s.sent[0].clone());
    assert_eq!(session_id, "p");
    let path = text.strip_prefix("look ").expect("caption first");
    assert!(path.ends_with(".jpg"));
    assert!(
        Path::new(path).starts_with(rig.dir.path("images")),
        "in the photo folder, never a path of the phone's choosing"
    );
    assert_eq!(std::fs::read(path).expect("file"), data);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(Path::new(path)), 0o600);
        assert_eq!(mode(&rig.dir.path("images")), 0o700);
    }
    let entry = rig.audit.recent().pop().expect("audit");
    assert_eq!(
        (entry.op.as_str(), entry.length),
        ("image", Some(data.len()))
    );
}

#[test]
fn refusals() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![
            session("p", Control::Job("job".into()), SessionState::Running),
            session("v", Control::ViewOnly, SessionState::Running),
        ]
    });
    let (connection, io, _) = rig.login("phone");
    let ask = |request: Value| {
        rig.send(&connection, &request.to_string());
        io.last_json()
    };
    let begin = |session: &str, bytes: usize| {
        ask(json!({"id":1,"op":"session.image.begin","sessionId":session,"bytes":bytes}))
    };
    assert_eq!(error_code(&begin("v", 10)), Some("view_only"));
    assert_eq!(
        error_code(&begin("p", MAX_BYTES + 1)),
        Some("invalid"),
        "over the size limit"
    );
    assert_eq!(error_code(&begin("p", 0)), Some("invalid"));
    assert_eq!(error_code(&begin("ghost", 10)), Some("not_found"));
    let chunk =
        ask(json!({"id":4,"op":"session.image.chunk","uploadId":"nope","seq":0,"data":"AAAA"}));
    assert_eq!(error_code(&chunk), Some("not_found"));
    // Not a picture: bytes that are not JPEG, PNG, GIF or WebP.
    let reply = upload(&rig, &connection, &io, "p", "", &[0x41; 5000]);
    assert_eq!(error_code(&reply), Some("invalid"));
    // Short: fewer bytes than announced.
    let short = data_of(&begin("p", 5000))["uploadId"]
        .as_str()
        .expect("id")
        .to_string();
    let reply = ask(json!({"id":6,"op":"session.image.end","uploadId":short}));
    assert_eq!(error_code(&reply), Some("invalid"));
    assert!(
        rig.backend.with(|s| s.sent.is_empty()),
        "nothing reached the session"
    );
    assert!(saved_files(&rig).is_empty(), "nothing was saved either");
}

#[test]
fn chunks_must_come_in_order_and_not_exceed_the_announced_size() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session(
            "p",
            Control::Job("job".into()),
            SessionState::Running,
        )]
    });
    let (connection, io, _) = rig.login("phone");
    rig.send(
        &connection,
        r#"{"id":1,"op":"session.image.begin","sessionId":"p","bytes":4}"#,
    );
    let id = data_of(&io.last_json())["uploadId"]
        .as_str()
        .expect("id")
        .to_string();
    let chunk = |seq: i64, data: &str| {
        rig.send(
            &connection,
            &json!({"id":2,"op":"session.image.chunk","uploadId":id,"seq":seq,"data":data})
                .to_string(),
        );
        error_code(&io.last_json()).map(String::from)
    };
    assert_eq!(
        chunk(1, "AAAA").as_deref(),
        Some("invalid"),
        "sequence 1 before 0"
    );
    assert_eq!(
        chunk(0, "AAAAAAAA").as_deref(),
        Some("invalid"),
        "6 bytes into a 4 byte upload"
    );
    assert_eq!(chunk(0, "not base64!").as_deref(), Some("invalid"));
    assert_eq!(
        chunk(0, "AAAA").as_deref(),
        None,
        "the upload is still open after the refusals"
    );
}

#[test]
fn another_connection_cannot_finish_someone_elses_upload() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session(
            "p",
            Control::Job("job".into()),
            SessionState::Running,
        )]
    });
    let (connection, io, _) = rig.login("phone");
    rig.send(
        &connection,
        r#"{"id":1,"op":"session.image.begin","sessionId":"p","bytes":4}"#,
    );
    let id = data_of(&io.last_json())["uploadId"]
        .as_str()
        .expect("id")
        .to_string();
    let (other, other_io, _) = rig.login("other-phone");
    rig.send(
        &other,
        &json!({"id":2,"op":"session.image.end","uploadId":id}).to_string(),
    );
    assert_eq!(error_code(&other_io.last_json()), Some("not_found"));
}

#[test]
fn the_path_is_quoted_when_it_has_spaces_and_the_caption_may_be_empty() {
    assert_eq!(ImageStore::message("", "/a b/c.jpg"), "\"/a b/c.jpg\"");
    assert_eq!(ImageStore::message("  hi  ", "/a/c.jpg"), "hi /a/c.jpg");
    assert_eq!(ImageStore::message("", "/a/it's.jpg"), "\"/a/it's.jpg\"");
}

#[test]
fn types_are_detected_by_content() {
    assert_eq!(
        ImageStore::detect(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0]),
        Some("png")
    );
    assert_eq!(ImageStore::detect(&[0xFF, 0xD8, 0xFF]), Some("jpg"));
    assert_eq!(ImageStore::detect(b"GIF89a"), Some("gif"));
    assert_eq!(ImageStore::detect(b"RIFF\0\0\0\0WEBP"), Some("webp"));
    assert_eq!(ImageStore::detect(b"not an image"), None);
    assert_eq!(ImageStore::detect(&[]), None);
}

// MARK: Hostile input

#[test]
fn a_name_that_climbs_out_of_the_folder_cannot_escape_it() {
    // The phone names no file: the session id and caption are text, the file name is made here. A
    // caption full of dots and slashes ends up in the message, never in a path.
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session(
            "p",
            Control::Job("job".into()),
            SessionState::Running,
        )]
    });
    let (connection, io, _) = rig.login("phone");
    let reply = upload(
        &rig,
        &connection,
        &io,
        "p",
        "../../../../etc/cron.d/evil",
        &jpeg(),
    );
    assert!(is_ok(&reply));
    let files = saved_files(&rig);
    assert_eq!(files.len(), 1);
    let name = files[0]
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    assert!(
        name.len() == "phone-12345678.jpg".len()
            && name.starts_with("phone-")
            && name.ends_with(".jpg")
            && !name.contains(['/', '\\']),
        "{name}"
    );
    assert_eq!(files[0].parent(), Some(rig.dir.path("images").as_path()));
    // A session id that climbs is just an unknown session.
    rig.send(
        &connection,
        r#"{"id":5,"op":"session.image.begin","sessionId":"../../p","bytes":10}"#,
    );
    assert_eq!(error_code(&io.last_json()), Some("not_found"));
}

#[test]
fn the_store_refuses_an_extension_that_is_not_plain() {
    let dir = TempDir::new("store-ext");
    let store = ImageStore::new(dir.path("photos"));
    for ext in ["../x", "jpg/../../x", "", "a b", "j.pg", "/etc/passwd"] {
        assert!(store.save(&[1, 2, 3], ext, 1.0e12).is_err(), "{ext:?}");
    }
    assert!(!dir.path("x").exists());
    let saved = store
        .save(&[1, 2, 3], "png", 1.0e12)
        .expect("plain extension");
    assert_eq!(saved.parent(), Some(dir.path("photos").as_path()));
}

#[cfg(unix)]
#[test]
fn a_folder_that_was_already_there_is_made_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("store-mode");
    let folder = dir.path("photos");
    std::fs::create_dir_all(&folder).expect("mkdir");
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    ImageStore::new(folder.clone())
        .save(&[1], "jpg", 1.0e12)
        .expect("saved");
    let mode = std::fs::metadata(&folder)
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn an_idle_expired_upload_is_swept() {
    let uploads = Uploads::default();
    uploads.begin(1, "s", 10, "", 0.0);
    uploads.sweep(EXPIRES_AFTER_MS - 1.0);
    assert_eq!(uploads.pending(), 1, "not yet");
    uploads.sweep(EXPIRES_AFTER_MS + 1.0);
    assert_eq!(
        uploads.pending(),
        0,
        "gone without another call from the phone"
    );
}

#[test]
fn with_acting_off_an_upload_is_not_even_begun() {
    let backend: Arc<dyn BridgeBackend> = Arc::new(LiveBackend::new(FakeSources, ActingOff));
    let (router, connection, io, _dir) = live_rig(backend);
    router.handle(
        &connection,
        &json!({"id":2,"op":"session.image.begin","sessionId":"p","bytes":1000}).to_string(),
    );
    let reply = io.last_json();
    assert_eq!(error_code(&reply), Some("invalid"), "{reply}");
    assert_eq!(reply["error"]["message"], json!(ACTING_OFF));
    assert_eq!(router.pending_uploads(), 0, "nothing held in memory");
}

#[test]
fn old_files_are_pruned_after_a_day() {
    let dir = TempDir::new("store-prune");
    let store = ImageStore::new(dir.path("photos"));
    let now = std::time::SystemTime::now();
    let ms = |t: std::time::SystemTime| {
        t.duration_since(std::time::UNIX_EPOCH)
            .expect("epoch")
            .as_millis() as f64
    };
    let old = store.save(&[1], "jpg", ms(now)).expect("old");
    let aging = store.save(&[2], "jpg", ms(now)).expect("aging");
    let day = std::time::Duration::from_secs(24 * 3600);
    set_modified(&old, now - day - std::time::Duration::from_secs(60));
    set_modified(&aging, now - day + std::time::Duration::from_secs(600));
    let fresh = store.save(&[3], "jpg", ms(now)).expect("fresh");
    assert!(!old.exists(), "older than a day: gone");
    assert!(aging.exists() && fresh.exists());
}

fn set_modified(path: &Path, when: std::time::SystemTime) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open");
    file.set_modified(when).expect("mtime");
}

#[test]
fn an_upload_not_finished_in_time_is_dropped_and_cannot_be_continued() {
    let uploads = Uploads::default();
    let id = uploads.begin(1, "p", 8, "", 0.0);
    assert_eq!(uploads.pending(), 1);
    assert_eq!(uploads.append(&id, 1, 0, &[1, 2, 3, 4], 1_000.0), Ok(()));
    let late = EXPIRES_AFTER_MS + 1.0;
    assert_eq!(
        uploads.append(&id, 1, 1, &[5, 6, 7, 8], late),
        Err(UploadError::Unknown)
    );
    assert_eq!(
        uploads.pending(),
        0,
        "the half-photo was freed, not just ignored"
    );
    // Finishing late is refused too, even with every byte there.
    let id = uploads.begin(1, "p", 4, "", 0.0);
    assert_eq!(uploads.append(&id, 1, 0, &[1, 2, 3, 4], 1_000.0), Ok(()));
    assert!(matches!(
        uploads.finish(&id, 1, late),
        Err(UploadError::Unknown)
    ));
    assert_eq!(uploads.pending(), 0);
}

#[test]
fn a_stale_upload_of_another_phone_is_swept_when_a_new_one_begins() {
    let uploads = Uploads::default();
    let _forgotten = uploads.begin(1, "p", 100, "", 0.0);
    let _other = uploads.begin(2, "p", 100, "", 1_000.0);
    assert_eq!(uploads.pending(), 2);
    let _third = uploads.begin(3, "p", 100, "", EXPIRES_AFTER_MS + 5_000.0);
    assert_eq!(uploads.pending(), 1, "both stale ones were dropped");
}

#[test]
fn a_new_begin_replaces_the_connections_old_upload_and_a_closed_connection_drops_it() {
    let uploads = Uploads::default();
    let first = uploads.begin(1, "p", 4, "", 0.0);
    let second = uploads.begin(1, "p", 4, "", 10.0);
    assert_eq!(uploads.pending(), 1);
    assert_eq!(
        uploads.append(&first, 1, 0, &[1], 20.0),
        Err(UploadError::Unknown)
    );
    assert_eq!(uploads.append(&second, 1, 0, &[1], 20.0), Ok(()));
    uploads.drop_connection(1);
    assert_eq!(uploads.pending(), 0);

    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session(
            "p",
            Control::Job("job".into()),
            SessionState::Running,
        )]
    });
    let (connection, io, _) = rig.login("phone");
    rig.send(
        &connection,
        r#"{"id":1,"op":"session.image.begin","sessionId":"p","bytes":4}"#,
    );
    assert!(is_ok(&io.last_json()));
    rig.router.closed(&connection);
    assert_eq!(
        rig.router.pending_uploads(),
        0,
        "the router drops a closed connection's photo"
    );
}

#[test]
fn a_huge_announcement_or_piece_is_refused_without_buffering() {
    let uploads = Uploads::default();
    let id = uploads.begin(1, "p", 4, "", 0.0);
    let piece = vec![0u8; 160 * 1024 + 1];
    assert_eq!(
        uploads.append(&id, 1, 0, &piece, 1.0),
        Err(UploadError::TooBig)
    );
    assert_eq!(
        uploads.append(&id, 1, 0, &[1, 2, 3, 4, 5], 1.0),
        Err(UploadError::TooBig)
    );
    assert_eq!(
        uploads.append(&id, 1, 0, &[1, 2, 3, 4], 1.0),
        Ok(()),
        "nothing of the refused pieces was kept"
    );
}

// MARK: Acting

struct FakeSources;

impl Sources for FakeSources {
    fn claude_sessions(&self) -> Vec<Session> {
        vec![session(
            "p",
            Control::Job("job".into()),
            SessionState::Running,
        )]
    }
    fn now_ms(&self) -> f64 {
        NOW_MS
    }
}

#[derive(Default)]
struct RecordingActing {
    sent: Mutex<Vec<String>>,
}

impl Acting for RecordingActing {
    fn send_message(&self, text: &str, _: &Session) -> Result<(), BridgeFailure> {
        self.sent.lock().expect("sent").push(text.to_string());
        Ok(())
    }
    fn send_now(&self, _: &Session) -> Result<(), BridgeFailure> {
        Ok(())
    }
    fn respond(&self, _: &PendingPrompt, _: &PromptResponse) -> Result<(), BridgeFailure> {
        Ok(())
    }
    fn new_session(&self, _: &Path, _: Option<&str>) -> Result<(), BridgeFailure> {
        Ok(())
    }
    fn take_over(&self, session: &Session, _: bool) -> Result<TakeoverOutcome, BridgeFailure> {
        Ok(TakeoverOutcome {
            session_id: session.id.clone(),
            stopped_pid: None,
        })
    }
}

/// A router over the live backend, with the acting side the test gives it.
fn live_rig(
    backend: Arc<dyn BridgeBackend>,
) -> (Router, Arc<super::Connection>, Arc<RecordingIo>, TempDir) {
    let dir = TempDir::new("photo-live");
    let devices = Arc::new(MemoryDevices::default());
    let hub = Arc::new(super::hub::Hub::new());
    let router = Router::new(RouterParts {
        backend,
        devices: devices.clone(),
        codes: Arc::new(MemoryCodes::default()),
        limiter: Arc::new(MemoryLimiter::default()),
        audit: Arc::new(MemoryAudit::default()),
        hub: Arc::clone(&hub),
        home: dir.root().to_path_buf(),
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
    (router, connection, io, dir)
}

fn send_photo(router: &Router, connection: &Arc<super::Connection>, io: &RecordingIo) -> Value {
    let data = jpeg();
    router.handle(
        connection,
        &json!({"id":2,"op":"session.image.begin","sessionId":"p","bytes":data.len()}).to_string(),
    );
    let id = data_of(&io.last_json())["uploadId"]
        .as_str()
        .expect("id")
        .to_string();
    for (n, piece) in data.chunks(96 * 1024).enumerate() {
        router.handle(
            connection,
            &json!({"id":3,"op":"session.image.chunk","uploadId":id,"seq":n,"data":b64(piece)})
                .to_string(),
        );
    }
    router.handle(
        connection,
        &json!({"id":4,"op":"session.image.end","uploadId":id}).to_string(),
    );
    io.last_json()
}

#[test]
fn acting_turned_off_mid_photo_refuses_it_and_no_file_is_left_behind() {
    /// On when the upload begins, off by the time the path is typed.
    struct TurnedOff;
    impl Acting for TurnedOff {
        fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure> {
            ActingOff.send_message(text, session)
        }
        fn send_now(&self, session: &Session) -> Result<(), BridgeFailure> {
            ActingOff.send_now(session)
        }
        fn respond(
            &self,
            prompt: &PendingPrompt,
            response: &PromptResponse,
        ) -> Result<(), BridgeFailure> {
            ActingOff.respond(prompt, response)
        }
        fn new_session(&self, cwd: &Path, model: Option<&str>) -> Result<(), BridgeFailure> {
            ActingOff.new_session(cwd, model)
        }
        fn take_over(
            &self,
            session: &Session,
            stop: bool,
        ) -> Result<TakeoverOutcome, BridgeFailure> {
            ActingOff.take_over(session, stop)
        }
    }
    let backend: Arc<dyn BridgeBackend> = Arc::new(LiveBackend::new(FakeSources, TurnedOff));
    let (router, connection, io, dir) = live_rig(backend);
    let reply = send_photo(&router, &connection, &io);
    assert_eq!(error_code(&reply), Some("invalid"));
    assert_eq!(reply["error"]["message"], json!(ACTING_OFF));
    let left = std::fs::read_dir(dir.path("images"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(left, 0, "the refused photo was removed");
}

#[test]
fn with_acting_on_the_path_goes_through_the_acting_trait() {
    let acting = Arc::new(RecordingActing::default());
    struct Shared(Arc<RecordingActing>);
    impl Acting for Shared {
        fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure> {
            self.0.send_message(text, session)
        }
        fn send_now(&self, session: &Session) -> Result<(), BridgeFailure> {
            self.0.send_now(session)
        }
        fn respond(
            &self,
            prompt: &PendingPrompt,
            response: &PromptResponse,
        ) -> Result<(), BridgeFailure> {
            self.0.respond(prompt, response)
        }
        fn new_session(&self, cwd: &Path, model: Option<&str>) -> Result<(), BridgeFailure> {
            self.0.new_session(cwd, model)
        }
        fn take_over(
            &self,
            session: &Session,
            stop: bool,
        ) -> Result<TakeoverOutcome, BridgeFailure> {
            self.0.take_over(session, stop)
        }
    }
    let backend: Arc<dyn BridgeBackend> =
        Arc::new(LiveBackend::new(FakeSources, Shared(acting.clone())));
    let (router, connection, io, dir) = live_rig(backend);
    assert!(is_ok(&send_photo(&router, &connection, &io)));
    let sent = acting.sent.lock().expect("sent").clone();
    assert_eq!(sent.len(), 1);
    assert!(
        Path::new(&sent[0]).starts_with(dir.path("images")),
        "{sent:?}"
    );
    assert!(Path::new(&sent[0]).exists());
}
