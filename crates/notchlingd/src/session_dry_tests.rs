use super::*;

fn kind_of(tool: &'static str, id: &str) -> Target {
    Target {
        tool,
        ..target(id, "/work/a")
    }
}

fn dry(id: &str, extra: Value) -> Value {
    let mut args = json!({"id": id, "dryRun": true});
    if let (Some(into), Some(from)) = (args.as_object_mut(), extra.as_object()) {
        into.extend(from.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    args
}

fn quiet(r: &Rig) {
    assert!(r.ops_run().is_empty(), "{:?}", r.ops_run());
    assert_eq!(r.cards(), 0);
}

#[test]
fn a_dry_run_on_a_free_path_reports_it_and_acts_on_nothing() {
    let r = rig("dry-free");
    r.make_child("kid", "parent");
    let mut seen = Vec::new();
    for (op, args) in [
        (ops::SESSION_SAY, dry("kid", json!({"text": TEXT}))),
        (ops::SESSION_HALT, dry("kid", json!({"how": "stop"}))),
        (ops::SESSION_HALT, dry("kid", json!({"how": "interrupt"}))),
        (ops::SESSION_HALT, dry("parent", json!({"how": "stop"}))),
        (
            ops::SESSION_HALT,
            dry("parent", json!({"how": "interrupt"})),
        ),
    ] {
        let said = r.call(op, args.clone()).unwrap();
        assert_eq!(said["dryRun"], json!(true), "{args}");
        assert_eq!(said["would"], "apply", "{args}");
        assert_eq!(said["op"], op, "{args}");
        assert_eq!(said["error"], Value::Null, "{args}");
        assert_eq!(said.get("free"), None, "{args}");
        seen.push(said);
    }
    assert_eq!(seen[0]["change"]["session"], "kid");
    assert_eq!(seen[0]["change"]["action"], "say");
    quiet(&r);
    assert_eq!(r.asks.children().get("kid").unwrap().stopped_ms, None);
}

#[test]
fn a_dry_run_remove_of_a_stopped_child_keeps_the_child() {
    let r = rig("dry-remove");
    r.make_child("kid", "parent");
    let early = r
        .call(ops::SESSION_DISCARD, dry("kid", json!({})))
        .unwrap_err();
    assert_eq!(early.code, ErrorCode::BadRequest);
    r.call(ops::SESSION_HALT, json!({"id": "kid", "how": "stop"}))
        .unwrap();
    let before = r.ops_run();
    let said = r.call(ops::SESSION_DISCARD, dry("kid", json!({}))).unwrap();
    assert_eq!(said["dryRun"], json!(true));
    assert_eq!(said["would"], "apply");
    assert_eq!(r.ops_run(), before, "nothing ran for the dry run");
    assert!(r.asks.children().get("kid").is_some());
    let own = r
        .call(ops::SESSION_DISCARD, dry("parent", json!({})))
        .unwrap_err();
    assert_eq!(own.code, ErrorCode::BadRequest);
}

#[test]
fn a_dry_run_never_takes_the_free_quota_and_still_says_when_it_is_used_up() {
    let r = rig("dry-quota");
    r.make_child("kid", "parent");
    for i in 0..FREE_PER_HOUR {
        r.clock.store(1_000 + i as i64, Ordering::SeqCst);
        let said = r
            .call(ops::SESSION_SAY, dry("kid", json!({"text": "hi"})))
            .unwrap();
        assert_eq!(said["would"], "apply");
    }
    quiet(&r);
    for i in 0..FREE_PER_HOUR {
        r.clock.store(2_000 + i as i64, Ordering::SeqCst);
        r.call(ops::SESSION_SAY, json!({"id": "kid", "text": "hi"}))
            .unwrap();
    }
    assert_eq!(r.ops_run().len(), FREE_PER_HOUR);
    r.clock.store(60_000, Ordering::SeqCst);
    let busy = r
        .call(ops::SESSION_SAY, dry("kid", json!({"text": "hi"})))
        .unwrap_err();
    assert_eq!(reason(&busy), "busy");
    assert!(field(&busy, "retryAfterMs").as_i64().unwrap() > 0);
    assert_eq!(r.ops_run().len(), FREE_PER_HOUR);
}

#[test]
fn a_dry_run_on_a_card_path_still_opens_no_card() {
    let r = rig("dry-card");
    for (op, args, kind) in [
        (
            ops::SESSION_SAY,
            dry("other", json!({"text": TEXT})),
            "session",
        ),
        (
            ops::SESSION_HALT,
            dry("other", json!({"how": "stop"})),
            "session_stop",
        ),
        (ops::SESSION_DISCARD, dry("other", json!({})), "session"),
    ] {
        let said = r.call(op, args).unwrap();
        assert_eq!(said["dryRun"], json!(true), "{op}");
        assert_eq!(said["would"], "ask", "{op}");
        assert_eq!(said["request"]["kind"], kind, "{op}");
    }
    quiet(&r);
}

#[test]
fn removing_an_antigravity_session_is_refused_before_any_card() {
    let r = rig("dry-agy-remove");
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("agy-kid".to_string(), kind_of("antigravity", "agy-kid"));
    for args in [
        json!({"id": "agy-kid"}),
        json!({"id": "agy-kid", "dryRun": true}),
    ] {
        let e = r.call(ops::SESSION_DISCARD, args.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{args}");
        assert_eq!(reason(&e), "not_yet", "{args}");
    }
    quiet(&r);
}

#[test]
fn a_remove_the_tool_supports_still_opens_its_card() {
    let r = rig("dry-codex-remove");
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("codex-kid".to_string(), kind_of("codex", "codex-kid"));
    let said = r
        .call(ops::SESSION_DISCARD, json!({"id": "codex-kid"}))
        .unwrap();
    let id = r.card_of(&said);
    assert_eq!(r.allow(&id, "mac").state.as_str(), "applied");
    assert_eq!(r.ops_run(), [ops::CODEX_DELETE]);
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("agy-kid".to_string(), kind_of("antigravity", "agy-kid"));
    let stop = r
        .call(ops::SESSION_HALT, json!({"id": "agy-kid", "how": "stop"}))
        .unwrap();
    r.card_of(&stop);
}
