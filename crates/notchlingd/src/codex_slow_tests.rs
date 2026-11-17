use super::*;

impl Setup {
    fn wait_turn_state(&self, state: &str) -> Value {
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            let found = self
                .room
                .named("codex.turn")
                .into_iter()
                .find(|e| e["state"] == state);
            if let Some(found) = found {
                return found;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no codex.turn {state}: {:?}", self.room.named("codex.turn"));
    }
}

#[test]
fn a_new_session_slow_to_be_named_is_answered_starting_and_named_later_with_its_draft() {
    let s = setup("slowdraft", true);
    s.ops.set_reply_wait(Duration::from_millis(100));
    s.ops.runs().set_announce_wait(Duration::from_secs(5));
    s.spawner.push(FakeRun::gated(&[STARTED, DONE]));
    let began = Instant::now();
    let reply = s
        .call(
            ops::CODEX_SEND,
            json!({"draft_folder": s.work(), "text": "build it", "draft": "codex-new:u1"}),
        )
        .expect("a reply, though the session has no name yet");
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "within the client's 5 s"
    );
    assert_eq!(reply, json!({"state": "starting", "draft": "codex-new:u1"}));
    s.spawner.processes()[0].release();
    let named = s.wait_turn_state("named");
    assert_eq!(
        named,
        json!({"id": format!("codex:{NEW_SESSION}"), "session": NEW_SESSION,
               "state": "named", "draft": "codex-new:u1"})
    );
    s.idle();
}

struct Links(std::sync::Mutex<Vec<(String, String)>>);

impl crate::codex_ops::JobLink for Links {
    fn session_started(&self, job: &str, session: &str) {
        self.0
            .lock()
            .unwrap()
            .push((job.to_string(), session.to_string()));
    }

    fn start_failed(&self, job: &str, why: &str) {
        self.0
            .lock()
            .unwrap()
            .push((job.to_string(), format!("failed: {why}")));
    }
}

#[test]
fn a_cluster_job_that_never_gets_a_thread_is_reported_failed() {
    let s = setup("failjob", true);
    s.ops.set_reply_wait(Duration::from_millis(100));
    s.ops.runs().set_announce_wait(Duration::from_secs(5));
    let links = Arc::new(Links(Default::default()));
    s.ops.set_job_link(links.clone());
    s.spawner.push(FakeRun::gated(&[]).exiting(1));
    let reply = s
        .call(
            ops::CODEX_SEND,
            json!({"draft_folder": s.work(), "text": "build it", "job": "job-9"}),
        )
        .expect("a reply");
    assert_eq!(reply["state"], "starting");
    s.spawner.processes()[0].release();
    let until = Instant::now() + Duration::from_secs(10);
    while links.0.lock().unwrap().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    let got = links.0.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, "job-9");
    assert!(got[0].1.starts_with("failed: "), "{got:?}");
    s.idle();
}

#[test]
fn a_cluster_job_started_slowly_links_its_session_when_the_name_comes_late() {
    let s = setup("slowjob", true);
    s.ops.set_reply_wait(Duration::from_millis(100));
    s.ops.runs().set_announce_wait(Duration::from_secs(5));
    let links = Arc::new(Links(Default::default()));
    s.ops.set_job_link(links.clone());
    s.spawner.push(FakeRun::gated(&[STARTED, DONE]));
    let reply = s
        .call(
            ops::CODEX_SEND,
            json!({"draft_folder": s.work(), "text": "build it", "job": "job-7"}),
        )
        .expect("a reply, though the session has no name yet");
    assert_eq!(reply["state"], "starting");
    assert!(links.0.lock().unwrap().is_empty());
    s.spawner.processes()[0].release();
    let until = Instant::now() + Duration::from_secs(10);
    while links.0.lock().unwrap().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        *links.0.lock().unwrap(),
        vec![("job-7".to_string(), format!("codex:{NEW_SESSION}"))]
    );
    s.idle();
}

#[test]
fn a_start_that_answers_in_time_needs_no_late_link() {
    let s = setup("fastjob", true);
    let links = Arc::new(Links(Default::default()));
    s.ops.set_job_link(links.clone());
    s.spawner.push(FakeRun::ok(&[STARTED, DONE]));
    let reply = s
        .call(
            ops::CODEX_SEND,
            json!({"draft_folder": s.work(), "text": "build it", "job": "job-8"}),
        )
        .expect("a reply");
    assert_eq!(reply["state"], "started");
    assert!(links.0.lock().unwrap().is_empty());
    s.idle();
}

#[test]
fn a_job_arg_is_a_short_text() {
    let s = setup("jobarg", true);
    let err = s
        .call(
            ops::CODEX_SEND,
            json!({"draft_folder": s.work(), "text": "x", "job": 5}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

#[test]
fn a_new_session_that_never_gets_named_says_so_in_an_event_with_its_draft() {
    let s = setup("faildraft", true);
    s.ops.set_reply_wait(Duration::from_millis(100));
    s.ops.runs().set_announce_wait(Duration::from_secs(5));
    s.spawner.push(FakeRun::gated(&[]).exiting(1));
    let reply = s
        .call(
            ops::CODEX_SEND,
            json!({"draft_folder": s.work(), "text": "build it", "draft": "codex-new:u2"}),
        )
        .expect("a reply");
    assert_eq!(reply["state"], "starting");
    s.spawner.processes()[0].release();
    let failed = s.wait_turn_state("not_started");
    assert_eq!(failed["draft"], "codex-new:u2");
    assert_eq!(failed["id"], Value::Null);
    assert!(
        failed["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{failed}"
    );
    s.idle();
}

#[test]
fn the_draft_name_is_a_codex_draft_row_and_only_for_a_new_session() {
    let s = setup("draftarg", true);
    for draft in [
        json!("agy-new:u1"),
        json!(7),
        json!("codex-new:".to_string() + &"x".repeat(100)),
    ] {
        let (c, _) = code(s.call(
            ops::CODEX_SEND,
            json!({"draft_folder": s.work(), "text": "hi", "draft": draft}),
        ));
        assert_eq!(c, ErrorCode::BadRequest, "{draft}");
    }
    let (c, _) = code(s.call(
        ops::CODEX_SEND,
        json!({"id": format!("codex:{SESSION}"), "text": "hi", "draft": "codex-new:u1"}),
    ));
    assert_eq!(c, ErrorCode::BadRequest, "a session already has its name");
    assert!(s.spawner.specs().is_empty());
}

#[test]
fn a_delete_that_takes_long_is_answered_deleting_and_reports_its_end_as_an_event() {
    let s = setup("slowdelete", true);
    s.ops.set_reply_wait(Duration::from_millis(100));
    s.launcher.set_delay(Duration::from_millis(600));
    let id = format!("codex:{SESSION}");
    let began = Instant::now();
    let reply = s
        .call(ops::CODEX_DELETE, json!({"id": id}))
        .expect("a reply");
    assert!(
        began.elapsed() < Duration::from_millis(550),
        "it did not wait for codex delete"
    );
    assert_eq!(reply, json!({"deleted": false, "state": "deleting"}));
    let done = s.wait_named("session.changed", 1);
    assert_eq!(done[0]["deleted"], json!(true));
    assert_eq!(done[0]["row"], Value::Null);
}

#[test]
fn a_slow_delete_that_fails_reports_the_failure_and_leaves_the_row_alone() {
    let s = setup("slowdeletefail", true);
    s.ops.set_reply_wait(Duration::from_millis(100));
    s.launcher.set_delay(Duration::from_millis(400));
    s.launcher.push_output(Ok(LaunchOutput {
        code: 3,
        output: String::new(),
        stderr: "locked\n".into(),
    }));
    let id = format!("codex:{SESSION}");
    let reply = s
        .call(ops::CODEX_DELETE, json!({"id": id}))
        .expect("a reply");
    assert_eq!(reply["state"], "deleting");
    let failed = s.wait_turn_state("delete_failed");
    assert_eq!(failed["id"], json!(id));
    assert_eq!(failed["error"], "locked");
    assert!(
        s.room
            .named("session.changed")
            .iter()
            .all(|e| e["deleted"] != json!(true)),
        "the row stays"
    );
}
