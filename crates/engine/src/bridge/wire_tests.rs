//! Port of `BridgeRouterTests`' subscription and wire-format cases.

use super::model::*;
use super::test_support::*;
use super::wire::{self, BridgeFailure};
use notchling_protocol::generated::ErrorCode;
use serde_json::{json, Value};

fn codes(io: &RecordingIo) -> Option<String> {
    error_code(&io.last_json()).map(String::from)
}

// MARK: Subscriptions

#[test]
fn subscribe_sends_sessions_now_and_clamps_phone_options() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)]);
    let (connection, io, _) = rig.login("test-phone");
    rig.send(&connection, r#"{"id":1,"op":"subscribe","sessions":true,"phone":{"fps":60,"width":4000,"deviceWindowId":12}}"#);
    assert_eq!(
        io.json(1)["event"],
        "sessions",
        "the first push comes at once"
    );
    let starts = rig.backend.with(|s| s.phone_starts.clone());
    assert_eq!(
        starts,
        [PhoneOptions {
            device_window_id: Some(12),
            fps: 15,
            width: 600
        }]
    );
    assert!(is_ok(&io.last_json()));
    assert!(connection.wants_sessions());

    rig.send(&connection, r#"{"id":2,"op":"unsubscribe"}"#);
    assert!(!connection.wants_sessions());
}

#[test]
fn a_phone_subscribe_error_is_reported_and_the_session_must_exist() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.phone_error = Some(BridgeFailure::internal("Chưa có quyền ghi màn hình")));
    let (connection, io, _) = rig.login("test-phone");
    rig.send(
        &connection,
        r#"{"id":1,"op":"subscribe","phone":{"fps":10}}"#,
    );
    assert_eq!(codes(&io).as_deref(), Some("internal"));
    rig.send(
        &connection,
        r#"{"id":2,"op":"subscribe","sessionId":"ghost"}"#,
    );
    assert_eq!(codes(&io).as_deref(), Some("not_found"));
}

// MARK: Wire format

#[test]
fn response_encoding_shape_and_id_echo() {
    let ok: Value =
        serde_json::from_str(&wire::ok(Some(&json!("abc")), json!({"n": 1}))).expect("json");
    assert_eq!(ok, json!({"id": "abc", "ok": true, "data": {"n": 1}}));
    let failure = BridgeFailure::view_only("chỉ xem");
    let refused: Value =
        serde_json::from_str(&wire::error(Some(&json!(9)), &failure)).expect("json");
    assert_eq!(
        refused,
        json!({"id": 9, "ok": false, "error": {"code": "view_only", "message": "chỉ xem"}})
    );
    assert_eq!(
        wire::ok(Some(&json!(1)), json!({"b": 2, "a": 1})),
        r#"{"data":{"a":1,"b":2},"id":1,"ok":true}"#,
        "sorted keys, stable text"
    );
    assert_eq!(wire::code_name(ErrorCode::NotReachable), "not_reachable");
    let mut fields = serde_json::Map::new();
    fields.insert("data".into(), json!([]));
    assert_eq!(
        wire::event("sessions", fields),
        r#"{"data":[],"event":"sessions"}"#
    );
    assert_eq!(
        wire::text(&json!({"t": 1_793_541_465_000.0_f64, "p": 12.5, "u": "a/b"})),
        r#"{"p":12.5,"t":1793541465000,"u":"a/b"}"#
    );
}

#[test]
fn phone_frame_header_and_drop_when_busy() {
    assert_eq!(
        wire::frame::phone(0x0102_0304, &[0xAA, 0xBB]),
        [b'P', b'H', b'N', b'1', 1, 2, 3, 4, 0xAA, 0xBB]
    );
    let rig = Rig::new();
    let (connection, io) = rig.connect("192.168.1.20");
    io.hold_binary
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(connection.offer_phone_frame(&[1]));
    assert!(
        !connection.offer_phone_frame(&[2]),
        "the first frame is still in flight"
    );
    assert!(!connection.offer_phone_frame(&[3]));
    assert_eq!(io.binaries().len(), 1, "never more than one frame buffered");
    assert_eq!(connection.frame_counts().0, 2);

    io.release_binary();
    assert!(connection.offer_phone_frame(&[4]));
    assert_eq!(
        &io.binaries()[1][4..8],
        [0, 0, 0, 2],
        "sequence counts frames actually sent"
    );
}
