use super::*;
use crate::acting_ops::now_ms;
use crate::agy_ops::tests::SESSION;
use crate::agy_sidecar_ops::{SidecarLink, NOT_RUNNING};
use notchling_engine::outbox::State;
use notchling_engine::settings::Settings;
use notchling_engine::tools::agy_sidecar::jobs::{Job as SidecarJob, Kind, Outcome, Patience};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const NEW: &str = "5ce2bd47-0c1e-4e6b-8f43-6aa0e2f1d3aa";

fn choose(a: &AgyBench, channel: &str) {
    let dir = std::env::temp_dir().join(format!(
        "nl-sidecar-route-{}-{}",
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

fn link(a: &AgyBench) -> Arc<SidecarLink> {
    let wait = Duration::from_millis(600);
    let link = SidecarLink::with(
        Box::new(now_ms),
        Patience {
            pickup: wait,
            run: wait,
        },
        Box::new(|_| true),
    );
    a.rig.ops.attach_sidecar(link.clone());
    link
}

fn running(link: &Arc<SidecarLink>) {
    link.queue().seen(now_ms());
}

fn sidecar(link: &Arc<SidecarLink>, outcome: Outcome) -> JoinHandle<Vec<SidecarJob>> {
    running(link);
    let link = link.clone();
    std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while got.is_empty() {
            for job in link.queue().take(Duration::from_millis(20)) {
                link.queue().report(&job.id, outcome.clone());
                got.push(job);
            }
            assert!(Instant::now() < end, "no job came");
        }
        got
    })
}

fn attempt_id(row: &Row) -> String {
    format!("{}-{}-{}", row.id, row.tries, row.attempt_ms.unwrap())
}

fn deliver(a: &AgyBench, text: &str) -> (TryEnd, Option<String>, String) {
    let row = a.b.send(ROW, text);
    let taken = a.b.outbox.store().take(&row.id, T0).unwrap().unwrap();
    let (end, _, note) = a.courier.deliver(&taken, false);
    (end, note, row.id)
}

#[test]
fn the_default_channel_never_touches_the_sidecar_queue() {
    let a = agy();
    let link = link(&a);
    running(&link);
    assert_eq!(a.rig.ops.channel().id(), "acp");
    let process = a.rig.loaded();
    let row = a.b.send(ROW, "Change hello.txt");
    a.b.postmen.postman(&row.session).pass(a.b.at(T0));
    assert_eq!(process.requests("session/prompt").len(), 1);
    assert_eq!(link.queue().waiting(), 0);
    assert!(link.queue().take(Duration::from_millis(5)).is_empty());
}

#[test]
fn a_reply_goes_through_the_sidecar_queue_under_the_outbox_row_id_and_agy_is_never_started() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    let taker = sidecar(&link, Outcome::Delivered { session: None });
    let (end, note, id) = deliver(&a, "hello there");
    assert_eq!(end, TryEnd::Ok, "{note:?}");
    let jobs = taker.join().unwrap();
    assert_eq!(jobs.len(), 1);
    let row = a.b.outbox.store().get(&id).unwrap().unwrap();
    assert_eq!(jobs[0].id, attempt_id(&row));
    assert_eq!(jobs[0].text, "hello there");
    assert_eq!(
        jobs[0].kind,
        Kind::Reply {
            session: SESSION.into()
        }
    );
    assert!(a.rig.spawner.calls().is_empty(), "no agy acp was started");
}

#[test]
fn with_no_sidecar_running_the_row_fails_visibly_and_nothing_is_queued() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    let (end, note, _) = deliver(&a, "hello");
    assert_eq!(end, TryEnd::AttachFailed);
    assert_eq!(note.as_deref(), Some(NOT_RUNNING));
    assert_eq!(link.queue().waiting(), 0);
    assert!(a.rig.spawner.calls().is_empty());
}

#[test]
fn a_delivery_the_steps_did_not_confirm_is_shown_as_unsure_not_as_sent() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    let taker = sidecar(&link, Outcome::Unconfirmed { session: None });
    let (end, _, _) = deliver(&a, "hello");
    assert_eq!(end, TryEnd::Cut);
    taker.join().unwrap();
}

#[test]
fn a_failure_the_sidecar_reports_keeps_its_words() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    let taker = sidecar(
        &link,
        Outcome::Failed("Antigravity has no conversation".into()),
    );
    let (end, note, _) = deliver(&a, "hello");
    assert_eq!(end, TryEnd::AttachFailed);
    assert_eq!(note.as_deref(), Some("Antigravity has no conversation"));
    taker.join().unwrap();
}

#[test]
fn a_phone_reply_is_allowed_and_queued_while_the_sidecar_is_chosen() {
    let a = agy();
    choose(&a, "sidecar");
    link(&a);
    let sent =
        a.b.outbox
            .call(
                &app(),
                ops::SESSION_SEND,
                &json!({"id": ROW, "text": "from the phone", "via": "phone"}),
            )
            .unwrap();
    assert_eq!(sent["state"], "queued");
}

#[test]
fn a_session_id_that_is_not_a_conversation_id_is_refused_for_the_sidecar() {
    let a = agy();
    choose(&a, "sidecar");
    assert!(a.rig.ops.check_session(SESSION).is_ok());
    assert!(a.rig.ops.check_session("../x").is_err());
    assert!(a.rig.ops.check_session("agy:abc").is_err());
    assert!(!a.rig.ops.is_busy(SESSION));
}

#[test]
fn a_new_conversation_goes_to_the_sidecar_with_a_checked_model_and_returns_its_id() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    let taker = sidecar(
        &link,
        Outcome::Delivered {
            session: Some(NEW.into()),
        },
    );
    let folder = a.rig.folder.to_string_lossy().into_owned();
    let reply = a
        .rig
        .call(
            ops::AGY_SEND,
            json!({"draft_folder": folder, "text": "Build the thing", "model": "flash"}),
        )
        .unwrap();
    assert_eq!(reply["id"], format!("agy:{NEW}"));
    assert_eq!(reply["sent"], true);
    let jobs = taker.join().unwrap();
    assert_eq!(jobs[0].text, "Build the thing");
    match &jobs[0].kind {
        Kind::Start { model, title } => {
            assert_eq!(model.as_deref(), Some("flash"));
            assert!(title.as_deref().is_some_and(|t| t.contains("Build")));
        }
        other => panic!("{other:?}"),
    }
    assert!(a.rig.spawner.calls().is_empty());
}

#[test]
fn a_new_conversation_with_a_flag_like_model_is_refused_without_queueing() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    running(&link);
    let folder = a.rig.folder.to_string_lossy().into_owned();
    for model in ["--model=x", "a b", "x;y", ""] {
        let (kind, _) = code(a.rig.call(
            ops::AGY_SEND,
            json!({"draft_folder": folder, "text": "hi", "model": model}),
        ));
        assert_eq!(kind, ErrorCode::BadRequest, "{model:?}");
    }
    assert_eq!(link.queue().waiting(), 0);
}

#[test]
fn a_new_conversation_with_no_sidecar_running_fails_and_starts_no_agy() {
    let a = agy();
    choose(&a, "sidecar");
    link(&a);
    let folder = a.rig.folder.to_string_lossy().into_owned();
    let (kind, message) = code(
        a.rig
            .call(ops::AGY_SEND, json!({"draft_folder": folder, "text": "hi"})),
    );
    assert_eq!(kind, ErrorCode::Failed);
    assert_eq!(message, NOT_RUNNING);
    assert!(a.rig.spawner.calls().is_empty());
}

#[test]
fn stop_is_a_soft_stop_for_the_sidecar_and_the_hard_cancel_is_unchanged_for_acp() {
    let a = agy();
    let link = link(&a);
    let cancelled = a.rig.call(ops::AGY_CANCEL, json!({"id": ROW})).unwrap();
    assert_eq!(cancelled, json!({"cancelled": true}));
    assert!(!link.stops().take(SESSION));

    choose(&a, "sidecar");
    let soft = a.rig.call(ops::AGY_CANCEL, json!({"id": ROW})).unwrap();
    assert_eq!(soft, json!({"cancelled": true, "soft": true}));
    assert!(link.stops().take(SESSION));
    assert!(!link.stops().take(SESSION));
}

#[test]
fn the_draft_channels_have_no_stop_at_all() {
    for channel in ["app", "remote"] {
        let a = agy();
        let link = link(&a);
        choose(&a, channel);
        let _ = a.rig.call(ops::AGY_CANCEL, json!({"id": ROW}));
        assert!(!link.stops().take(SESSION), "{channel}");
    }
}

#[test]
fn send_again_on_an_unsure_row_really_sends_a_new_job_while_the_same_attempt_never_does() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    let taker = sidecar(&link, Outcome::Unconfirmed { session: None });
    let (end, _, id) = deliver(&a, "hello");
    assert_eq!(end, TryEnd::Cut);
    let first = taker.join().unwrap()[0].id.clone();
    assert_eq!(
        first,
        attempt_id(&a.b.outbox.store().get(&id).unwrap().unwrap())
    );

    let taken = a.b.outbox.store().get(&id).unwrap().unwrap();
    let (again, _, _) = a.courier.deliver(&taken, false);
    assert_eq!(again, TryEnd::Cut);
    assert_eq!(link.queue().waiting(), 0);

    a.b.outbox
        .store()
        .requeue(&id, &[State::Sending], false, T0)
        .unwrap();
    let taker = sidecar(&link, Outcome::Delivered { session: None });
    let retaken = a.b.outbox.store().take(&id, T0 + 1_000).unwrap().unwrap();
    assert_eq!(retaken.tries, 2);
    let (end, note, _) = a.courier.deliver(&retaken, false);
    assert_eq!(end, TryEnd::Ok, "{note:?}");
    let jobs = taker.join().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, attempt_id(&retaken));
    assert_ne!(jobs[0].id, first);
}

#[test]
fn an_edited_unsure_row_is_a_new_attempt_even_though_its_tries_start_again() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    let taker = sidecar(&link, Outcome::Unconfirmed { session: None });
    let (end, _, id) = deliver(&a, "first words");
    assert_eq!(end, TryEnd::Cut);
    let first = taker.join().unwrap().remove(0);
    let store = a.b.outbox.store();
    store
        .set_state(&id, &[State::Sending], State::Unsure, None, None, T0 + 10)
        .unwrap();
    store
        .requeue(&id, &[State::Unsure], false, T0 + 15)
        .unwrap();
    store.edit(&id, "edited words", T0 + 20).unwrap();
    let retaken = store.take(&id, T0 + 1_000).unwrap().unwrap();
    assert_eq!(retaken.tries, 1);
    assert_eq!(retaken.text, "edited words");
    let taker = sidecar(&link, Outcome::Delivered { session: None });
    let (end, note, _) = a.courier.deliver(&retaken, false);
    assert_eq!(end, TryEnd::Ok, "{note:?}");
    let second = taker.join().unwrap().remove(0);
    assert_ne!(second.id, first.id);
    assert_eq!(second.text, "edited words");
    assert_eq!(first.text, "first words");
}

#[test]
fn a_stop_asked_before_a_new_message_goes_in_does_not_end_the_next_turn() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    a.rig.call(ops::AGY_CANCEL, json!({"id": ROW})).unwrap();
    let taker = sidecar(&link, Outcome::Delivered { session: None });
    let (end, note, _) = deliver(&a, "next question");
    assert_eq!(end, TryEnd::Ok, "{note:?}");
    taker.join().unwrap();
    assert!(!link.stops().take(SESSION));
}

#[test]
fn a_stop_asked_before_a_new_conversation_starts_is_forgotten_for_its_id() {
    let a = agy();
    choose(&a, "sidecar");
    let link = link(&a);
    link.stops().ask(NEW);
    let taker = sidecar(
        &link,
        Outcome::Delivered {
            session: Some(NEW.into()),
        },
    );
    let folder = a.rig.folder.to_string_lossy().into_owned();
    a.rig
        .call(
            ops::AGY_SEND,
            json!({"draft_folder": folder, "text": "Build"}),
        )
        .unwrap();
    taker.join().unwrap();
    assert!(!link.stops().take(NEW));
}
