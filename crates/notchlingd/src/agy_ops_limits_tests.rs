//! The limits on what the Antigravity ops take and give: the 1 MiB line, the message cap, the
//! summaries copy shared for a moment, a new session's folder, and acting turned off under them.

use super::*;

#[test]
fn the_session_list_shares_one_copy_of_the_database_for_a_few_seconds() {
    let rig = rig(true);
    assert_eq!(
        rig.call(SESSIONS, json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let copy = rig.copies.join("summaries.db");
    assert!(copy.is_file(), "the copy is made in the daemon's folder");
    let made = std::fs::metadata(&copy).unwrap().modified().unwrap();
    // A row Antigravity writes now is not seen until the copy is remade; asking again (the list,
    // the index and a folder lookup all do) does not copy the database and its WAL again.
    let db = Connection::open(rig.root.join(summaries::DB_NAME)).unwrap();
    db.execute(
        "INSERT INTO conversation_summaries SELECT 'later', title, preview, workspace_uris, status, \
         last_modified_time, nesting_depth FROM conversation_summaries LIMIT 1",
        [],
    )
    .unwrap();
    assert_eq!(
        rig.call(SESSIONS, json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(std::fs::metadata(&copy).unwrap().modified().unwrap(), made);
}

#[test]
fn a_long_conversation_comes_back_as_its_newest_items_and_says_so() {
    // A session of 1,500 exchanges of 1 KB each: 3 MB of conversation, over the 1 MiB line.
    let responder: notchling_engine::tools::acp::fake::Responder =
        Arc::new(|m: &Value, p: &FakeHandle| {
            let reply =
                |result: Value| p.push(&json!({"jsonrpc": "2.0", "id": m["id"], "result": result}));
            let update = |kind: &str, text: String| {
                p.push(
                    &json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                    "sessionId": SESSION,
                    "update": {"sessionUpdate": kind, "content": {"type": "text", "text": text}}}}),
                );
            };
            match m["method"].as_str() {
                Some("initialize") => reply(json!({"protocolVersion": 1})),
                Some("session/load") => {
                    for n in 0..1_500 {
                        update(
                            "user_message_chunk",
                            format!("question {n} {}", "q".repeat(1000)),
                        );
                        update(
                            "agent_message_chunk",
                            format!("answer {n} {}", "a".repeat(1000)),
                        );
                    }
                    reply(json!({}));
                }
                _ => {}
            }
        });
    let rig = rig_with(
        true,
        FakeSpawner::with_responder(responder),
        Some("/fake/agy"),
    );
    rig.loaded();
    let read = rig.call(CONVERSATION, json!({"id": ROW})).unwrap();
    let size = read.to_string().len();
    assert!(size < 600 * 1024, "{size} bytes");
    assert_eq!(read["truncated"], true);
    let omitted = read["omitted"].as_u64().unwrap() as usize;
    let conversation: notchling_engine::transcript::model::Conversation =
        serde_json::from_value(read["conversation"].clone()).unwrap();
    assert_eq!(conversation.items.len() + omitted, 3_000);
    let last = conversation.items.last().unwrap();
    assert!(
        matches!(&last.kind, notchling_engine::transcript::model::ItemKind::Assistant { text } if text.starts_with("answer 1499")),
        "the newest item is kept"
    );
    // A short one says it was not cut.
    let short = rig_with(true, FakeSpawner::replaying(RECORDING), Some("/fake/agy"));
    short.loaded();
    let read = short.call(CONVERSATION, json!({"id": ROW})).unwrap();
    assert_eq!(read["truncated"], false);
    assert_eq!(read["omitted"], 0);
}

#[test]
fn a_permission_with_a_huge_edit_goes_out_trimmed_in_its_events_and_lists() {
    let rig = rig(true);
    let process = rig.loaded();
    // About 900 KB of new text: it passes the 1 MiB line the agent's output is read in, and is far
    // over what a card, an event or `prompts.list` may carry.
    let new: String = (0..15_000)
        .map(|i| format!("line {i} {}\n", "z".repeat(50)))
        .collect();
    process.push(
        &json!({"jsonrpc": "2.0", "method": "session/request_permission", "id": "big",
        "params": {
            "toolCall": {"toolCallId": "c", "name": "edit_file", "kind": "edit",
                "title": "File big.txt", "rawInput": {"CommandLine": "c".repeat(200_000)},
                "content": [{"type": "diff", "path": "big.txt", "newText": new, "oldText": ""}]},
            "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}]}}),
    );
    let pending = rig.wait(PERMISSION_PENDING);
    let limit = 300 * 1024;
    assert!(pending.to_string().len() < limit, "the event is one line");
    let diff = pending["diff"].as_array().unwrap();
    assert_eq!(diff.last().unwrap()["kind"], "hunk");
    assert!(diff.last().unwrap()["text"]
        .as_str()
        .unwrap()
        .contains("more lines"));
    assert!(pending["input"].as_str().unwrap().len() < 8_000);
    let listed = rig.ops.prompts();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].to_string().len() < limit);
    // The state read carries the same trimmed card.
    let state = rig.call(STATE, json!({"id": ROW})).unwrap();
    assert!(state["permission"].to_string().len() < limit);
    // It can still be answered by its ids.
    let answer = rig
        .call(
            PERMISSION,
            json!({"id": ROW, "rpc_id": "big", "option_id": "allow"}),
        )
        .unwrap();
    assert_eq!(answer["outcome"], "allow_once");
}

#[test]
fn a_message_over_the_cap_is_refused_before_anything_starts() {
    use notchling_engine::tools::codex::plan::MAX_PROMPT_BYTES;
    let rig = rig(true);
    let folder = rig.folder.to_string_lossy().into_owned();
    let big = "x".repeat(MAX_PROMPT_BYTES + 1);
    for args in [
        json!({"id": ROW, "text": big}),
        json!({"draft_folder": folder, "text": big}),
    ] {
        let (code, message) = refused(rig.call(SEND, args));
        assert_eq!(code, ErrorCode::BadRequest);
        assert!(message.contains("KB"), "{message}");
    }
    assert!(rig.spawner.calls().is_empty(), "no agy was started");
    // At the cap it goes.
    let fits = "x".repeat(MAX_PROMPT_BYTES);
    rig.call(SEND, json!({"id": ROW, "text": fits})).unwrap();
}

/// An agent that starts a session `fresh-1` and ends every turn at once.
fn new_session_responder() -> notchling_engine::tools::acp::fake::Responder {
    Arc::new(|m: &Value, p: &FakeHandle| {
        let reply =
            |result: Value| p.push(&json!({"jsonrpc": "2.0", "id": m["id"], "result": result}));
        match m["method"].as_str() {
            Some("initialize") => reply(json!({"protocolVersion": 1})),
            Some("session/new") => reply(json!({"sessionId": "fresh-1"})),
            Some("session/prompt") => reply(json!({"stopReason": "end_turn"})),
            _ => {}
        }
    })
}

#[test]
fn a_new_session_never_starts_in_the_root_or_the_home_folder() {
    let rig = rig_with(
        true,
        FakeSpawner::with_responder(new_session_responder()),
        Some("/fake/agy"),
    );
    let home = std::fs::canonicalize(&rig.ops.setup.home).unwrap();
    for folder in ["/".to_string(), home.to_string_lossy().into_owned()] {
        let (code, message) =
            refused(rig.call(SEND, json!({"draft_folder": folder, "text": "hi"})));
        assert_eq!(code, ErrorCode::BadRequest, "{folder}");
        assert!(message.contains("root or the home"), "{message}");
    }
    // A link to the home is the home.
    #[cfg(unix)]
    {
        let link = rig.folder.join("home-link");
        std::os::unix::fs::symlink(&home, &link).unwrap();
        let (code, message) = refused(rig.call(
            SEND,
            json!({"draft_folder": link.to_string_lossy(), "text": "hi"}),
        ));
        assert_eq!(code, ErrorCode::BadRequest);
        assert!(message.contains("root or the home"), "{message}");
    }
    assert!(rig.spawner.calls().is_empty());
    // A folder inside the home is fine.
    let inside = home.join("project");
    std::fs::create_dir_all(&inside).unwrap();
    rig.call(
        SEND,
        json!({"draft_folder": inside.to_string_lossy(), "text": "hi"}),
    )
    .unwrap();
    assert_eq!(rig.spawner.calls().len(), 1);
}

#[test]
fn a_draft_that_loses_the_race_with_acting_off_spawns_nothing_more() {
    // `session/new` is answered, and acting goes off as the reply is made; the first message must
    // not reopen the session it would be sent to.
    let setup = setup(true, &[], json!([]));
    let acting = setup.ops.acting_switch();
    let flip = acting.clone();
    let inner = new_session_responder();
    let responder: notchling_engine::tools::acp::fake::Responder =
        Arc::new(move |m: &Value, p: &FakeHandle| {
            if m["method"].as_str() == Some("session/new") {
                flip.set(false);
            }
            inner(m, p);
        });
    let spawner = Arc::new(FakeSpawner::with_responder(responder));
    let folder = scratch("work");
    let agy = AgyOps::new(
        acting,
        setup.room.clone(),
        spawner.clone(),
        AgySetup {
            root: antigravity_root(&folder),
            copy_dir: scratch("copy"),
            home: scratch("home"),
            binary: Some("/fake/agy".into()),
            start_wait: Duration::from_secs(10),
            timing: Timing::default(),
        },
    );
    let result = agy.call(
        SEND,
        &json!({"draft_folder": folder.to_string_lossy(), "text": "hi"}),
    );
    assert_eq!(
        refused(result),
        (ErrorCode::Forbidden, ACTING_OFF.to_string())
    );
    assert_eq!(
        spawner.spawned().len(),
        1,
        "no second process for the prompt"
    );
    assert!(spawner.spawned()[0].killed(), "and the first is gone");
}

#[test]
fn turning_acting_off_closes_every_session_and_cancels_what_waited() {
    let setup = setup(true, &[], json!([]));
    let folder = scratch("work");
    let agy = Arc::new(AgyOps::new(
        setup.ops.acting_switch(),
        setup.room.clone(),
        Arc::new(FakeSpawner::replaying(RECORDING)),
        AgySetup {
            root: antigravity_root(&folder),
            copy_dir: scratch("copy"),
            home: scratch("home"),
            binary: Some("/fake/agy".into()),
            start_wait: Duration::from_secs(10),
            timing: Timing::default(),
        },
    ));
    setup.ops.attach_agy(agy.clone());
    agy.call(SEND, &json!({"id": ROW, "text": "edit"})).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while agy.prompts().is_empty() {
        assert!(Instant::now() < end, "no permission came");
        std::thread::sleep(Duration::from_millis(5));
    }
    // prompts.list shows it beside the hook's.
    let listed = setup
        .ops
        .call(&app(), ops::PROMPTS_LIST, &json!({}))
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["tool"], "agy");

    setup
        .ops
        .call(&app(), ops::SETTINGS_SET_ACTING, &json!({"on": false}))
        .unwrap();
    assert!(agy.prompts().is_empty());
    let resolved = setup.room.named(PERMISSION_RESOLVED);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["outcome"], "cancelled");
    assert_eq!(
        refused(agy.call(SEND, &json!({"id": ROW, "text": "again"}))).1,
        ACTING_OFF
    );
    assert_eq!(
        refused(agy.call(
            PERMISSION,
            &json!({"id": ROW, "rpc_id": "1", "option_id": "allow"})
        ))
        .1,
        ACTING_OFF
    );
    let state = agy.call(STATE, &json!({"id": ROW})).unwrap();
    assert_eq!(state["open"], false, "the process is gone");
}
