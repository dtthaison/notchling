//! The Codex ops while the `acting` switch is off, and when it is turned off under a running turn.

use super::*;

#[test]
fn the_ops_are_app_tier_and_refused_while_acting_is_off() {
    let s = setup("off", false);
    for op in CODEX_OPS {
        assert_eq!(s.ops.spec(op), Some(OpSpec::APP), "{op}");
        let args = json!({"id": format!("codex:{SESSION}"), "text": "hi", "draft_folder": "/tmp"});
        assert_eq!(
            code(s.ops.call(op, &args)),
            (ErrorCode::Forbidden, ACTING_OFF.to_string()),
            "{op}"
        );
    }
    assert_eq!(s.ops.spec("codex.nope"), None);
    assert!(s.spawner.specs().is_empty(), "nothing started");
    assert!(s.launcher.calls().is_empty(), "nothing ran");
    assert!(s.room.events.lock().unwrap().is_empty());
    // Turning it on lets the same call through.
    s.acting.set(true);
    s.spawner.push(FakeRun::ok(&[DONE]));
    assert!(s
        .call(
            ops::CODEX_SEND,
            json!({"id": format!("codex:{SESSION}"), "text": "hi"})
        )
        .is_ok());
    s.idle();
}

/// The M3 ops sharing the Codex setup's switch, with the Codex ops attached as the daemon does.
fn acting_ops(s: &Setup) -> crate::acting_ops::ActingOps {
    let ops = crate::acting_ops::ActingOps::new(
        s.acting.clone(),
        Arc::new(std::sync::Mutex::new(json!([]))),
        notchling_engine::sessions::Roots {
            main_dir: s.dir.0.join("claude"),
            lanes_file: s.dir.0.join("lanes.json"),
        },
        s.room.clone(),
        Box::new(crate::acting_ops::NoTerminal),
    );
    ops.attach_codex(s.ops.clone());
    ops
}

#[test]
fn turning_acting_off_stops_running_turns_and_drops_what_waited() {
    let s = setup("actingoff", true);
    let acting = acting_ops(&s);
    s.spawner.push(FakeRun::held(&[]));
    s.spawner.push(FakeRun::ok(&[DONE]));
    let id = format!("codex:{SESSION}");
    s.call(ops::CODEX_SEND, json!({"id": id, "text": "one"}))
        .expect("sent");
    s.call(ops::CODEX_SEND, json!({"id": id, "text": "two"}))
        .expect("queued");
    acting
        .call(
            &crate::acting_ops_tests::app(),
            ops::SETTINGS_SET_ACTING,
            &json!({"on": false}),
        )
        .expect("off");
    s.idle();
    assert_eq!(
        s.spawner.processes()[0].interrupts(),
        1,
        "the turn is stopped"
    );
    assert_eq!(
        s.spawner.specs().len(),
        1,
        "the message that waited never starts"
    );
    assert_eq!(s.ops.runs().waiting(SESSION), 0);
    assert!(s.ops.runs().running().is_empty());
}

#[test]
fn a_start_that_loses_the_race_with_acting_off_is_stopped() {
    // `set_acting(false)` lands while the process is being started: `stop_all` found nothing, so
    // the start notices the switch itself.
    let s = setup("racing", true);
    let acting = Arc::new(acting_ops(&s));
    s.spawner.push(FakeRun::held(&[]));
    let (flip, handle) = (acting.clone(), s.spawner.clone());
    handle.on_spawn(Box::new(move || {
        flip.call(
            &crate::acting_ops_tests::app(),
            ops::SETTINGS_SET_ACTING,
            &json!({"on": false}),
        )
        .expect("off");
    }));
    s.call(
        ops::CODEX_SEND,
        json!({"id": format!("codex:{SESSION}"), "text": "one"}),
    )
    .expect("sent");
    s.idle();
    assert_eq!(s.spawner.processes()[0].interrupts(), 1);
}
