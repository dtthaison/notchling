use super::*;
use crate::bridge_backend::send_failure;
use notchling_engine::bridge::wire::BridgeFailure;
use notchling_engine::settings::Settings;
use notchling_local_api::ApiError;

fn choose(a: &AgyBench, channel: &str) {
    let dir = std::env::temp_dir().join(format!(
        "nl-channel-{}-{}",
        channel,
        a.rig.folder.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    Settings::open(&path)
        .unwrap()
        .set("antigravityChannel", json!(channel))
        .unwrap();
    a.rig.ops.set_settings(path);
}

fn send(a: &AgyBench, via: Option<&str>) -> Result<Value, ApiError> {
    let mut args = json!({"id": ROW, "text": "hi"});
    if let Some(via) = via {
        args["via"] = json!(via);
    }
    a.b.outbox.call(&app(), ops::SESSION_SEND, &args)
}

#[test]
fn without_a_setting_the_agy_channel_is_used_and_nothing_changes() {
    let a = agy();
    assert_eq!(a.rig.ops.channel().id(), "acp");
    let sent = send(&a, None).unwrap();
    assert_eq!(sent["state"], "queued");
    let phone = send(&a, Some("phone")).unwrap();
    assert_eq!(phone["state"], "queued");
    assert!(!a.b.outbox.store().list(Some(ROW)).unwrap().is_empty());
}

#[test]
fn the_acp_choice_is_the_same_as_no_choice() {
    let a = agy();
    choose(&a, "acp");
    assert_eq!(a.rig.ops.channel().id(), "acp");
    assert_eq!(send(&a, Some("phone")).unwrap()["state"], "queued");
    let reply = a
        .rig
        .call(ops::AGY_SEND, json!({"id": ROW, "text": "hi"}))
        .unwrap();
    assert_eq!(reply["sent"], true);
}

#[test]
fn an_unknown_choice_falls_back_to_acp() {
    let a = agy();
    let dir = std::env::temp_dir().join(format!("nl-channel-junk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(
        &path,
        r#"{"version":1,"values":{"antigravityChannel":"junk"}}"#,
    )
    .unwrap();
    a.rig.ops.set_settings(path);
    assert_eq!(a.rig.ops.channel().id(), "acp");
}

#[test]
fn the_draft_channels_send_nothing_through_acp() {
    for channel in ["app", "remote"] {
        let a = agy();
        choose(&a, channel);
        assert_eq!(a.rig.ops.channel().id(), channel);
        let (kind, message) = code(send(&a, None));
        assert_eq!(kind, ErrorCode::BadRequest, "{channel}");
        assert!(message.contains("open in another app"), "{message}");
        let (kind, _) = code(a.rig.call(ops::AGY_SEND, json!({"id": ROW, "text": "hi"})));
        assert_eq!(kind, ErrorCode::BadRequest, "{channel}");
        let folder = a.rig.folder.to_string_lossy().into_owned();
        let (kind, _) = code(
            a.rig
                .call(ops::AGY_SEND, json!({"draft_folder": folder, "text": "hi"})),
        );
        assert_eq!(kind, ErrorCode::BadRequest, "{channel}");
        assert!(a.b.outbox.store().sessions().unwrap().is_empty());
        assert!(a.rig.spawner.calls().is_empty(), "no agy was started");
    }
}

#[test]
fn a_phone_reply_gets_the_view_only_refusal_while_a_draft_channel_is_chosen() {
    for channel in ["app", "remote"] {
        let a = agy();
        choose(&a, channel);
        let e = send(&a, Some("phone")).unwrap_err();
        assert!(
            e.message.contains("can't be used from the phone"),
            "{}",
            e.message
        );
        assert_eq!(e.data.as_ref().unwrap()["why"], "channel");
        let message = e.message.clone();
        assert_eq!(send_failure(e), BridgeFailure::view_only(message));
    }
}

#[test]
fn other_errors_keep_their_phone_shape() {
    let e = ApiError::new(ErrorCode::Failed, "gone");
    assert_eq!(send_failure(e), BridgeFailure::not_reachable("gone"));
    let e = ApiError::bad_request("nope");
    assert_eq!(send_failure(e), BridgeFailure::invalid("nope"));
}

#[test]
fn a_message_queued_before_the_switch_fails_instead_of_running_agy() {
    let a = agy();
    let row = a.b.send(ROW, "queued early");
    choose(&a, "app");
    let taken = a.b.outbox.store().take(&row.id, T0).unwrap().unwrap();
    let (end, _, note) = a.courier.deliver(&taken, false);
    assert_eq!(end, TryEnd::AttachFailed);
    assert!(note.unwrap().contains("open in another app"));
    assert!(a.rig.spawner.calls().is_empty());
}
