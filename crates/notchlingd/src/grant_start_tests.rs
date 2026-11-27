use super::*;

struct Starts {
    reply: Result<Value, ApiError>,
    seen: Mutex<Vec<String>>,
}

impl LocalCalls for Starts {
    fn call(&self, op: &str, _args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        self.seen.lock().unwrap().push(op.to_string());
        self.reply.clone()
    }
}

fn starts(reply: Result<Value, ApiError>) -> Arc<Starts> {
    Arc::new(Starts {
        reply,
        seen: Mutex::new(Vec::new()),
    })
}

fn from_home() -> Origin {
    from("vault", OriginTier::App)
}

#[cfg(unix)]
#[test]
fn a_claude_start_op_from_the_home_turns_the_pending_grant_into_the_jobs_grant() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    let inner = starts(Ok(json!({"jobId": "c1"})));
    let calls = UseCalls::new(inner.clone(), rig.uses.clone());
    let args = json!({"folder": "/work", "job": JOB});
    calls
        .call(ops::SESSION_NEW, &args, &from_home())
        .expect("started");
    assert_eq!(*inner.seen.lock().unwrap(), [ops::SESSION_NEW]);
    assert!(rig.roots.is_live(JOB));
    assert!(rig.roots.ran_claude(JOB), "session.new starts Claude Code");
    assert!(!rig.roots.is_pending(JOB, real_now()));
    assert_eq!(rig.roots.keys(JOB), ["c1".to_string()]);
    let janitor = janitor(&rig);
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"), "a started job keeps its grant");
    assert_eq!(rig.marks.marked()[0].keys, ["c1".to_string()]);
    assert_eq!(rig.masks.job_for_key("c1"), Some(JOB.to_string()));
    assert_eq!(rig.marks.marked()[0].ended_ms, None);
}

#[cfg(unix)]
#[test]
fn an_antigravity_start_op_is_noted_the_same_way() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    let calls = UseCalls::new(starts(Ok(json!({"id": "agy:1"}))), rig.uses.clone());
    let args = json!({"draft_folder": "/work", "job": JOB, "text": "go"});
    calls
        .call(ops::AGY_SEND, &args, &from_home())
        .expect("started");
    assert!(rig.roots.is_live(JOB));
    assert!(!rig.roots.ran_claude(JOB), "agy.send isn't Claude Code");
    assert_eq!(rig.roots.keys(JOB), ["agy:1".to_string()]);
}

#[test]
fn a_job_that_began_with_both_tools_ran_claude_and_its_end_forgets_it() {
    let rig = rig();
    assert!(!rig.roots.ran_claude(JOB));
    assert!(rig.roots.begin(JOB, StartTool::Agy));
    assert!(!rig.roots.ran_claude(JOB));
    assert!(rig.roots.begin(JOB, StartTool::Claude));
    assert!(rig.roots.begin(JOB, StartTool::Agy));
    assert!(rig.roots.ran_claude(JOB), "one Claude start is enough");
    rig.roots.end(JOB);
    assert!(!rig.roots.ran_claude(JOB));
    assert!(!rig.roots.begin(JOB, StartTool::Claude), "ended for good");
    assert!(!rig.roots.ran_claude(JOB));
}

#[test]
fn only_the_two_start_ops_name_a_tool() {
    assert_eq!(start_tool(ops::SESSION_NEW), Some(StartTool::Claude));
    assert_eq!(start_tool(ops::AGY_SEND), Some(StartTool::Agy));
    for other in ["shell.run", "codex.send", "sessions.list", "grant.deliver"] {
        assert_eq!(start_tool(other), None, "{other}");
    }
}

#[cfg(unix)]
#[test]
fn a_start_op_that_fails_ends_the_job_and_removes_its_grants() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    let failing = starts(Err(ApiError::new(ErrorCode::Failed, "no trust")));
    let calls = UseCalls::new(failing, rig.uses.clone());
    let args = json!({"folder": "/work", "job": JOB});
    let err = calls
        .call(ops::SESSION_NEW, &args, &from_home())
        .expect_err("the op's own error comes back");
    assert_eq!(err.message, "no trust");
    assert!(!has(&rig, JOB, "api-key"));
    assert!(!rig.roots.is_live(JOB));
    assert!(!rig.roots.is_pending(JOB, real_now()));
    assert!(rig.roots.is_ended(JOB));
    let err = deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect_err("ended");
    assert!(err.message.contains("isn't running"));
}

#[test]
fn a_start_op_that_does_not_come_from_the_home_with_a_job_is_not_noted() {
    let rig = rig();
    let calls = UseCalls::new(starts(Ok(json!({"jobId": "c1"}))), rig.uses.clone());
    let args = json!({"folder": "/work", "job": JOB});
    for origin in [
        from("other", OriginTier::App),
        from("vault", OriginTier::Agent),
        from("vault", OriginTier::Node),
    ] {
        calls
            .call(ops::SESSION_NEW, &args, &origin)
            .expect("still runs");
    }
    for bad in [json!({"folder": "/work"}), json!({"job": "j-nothex"})] {
        calls
            .call(ops::SESSION_NEW, &bad, &from_home())
            .expect("still runs");
    }
    assert!(!rig.roots.is_live(JOB));
    assert!(rig.roots.rooted().is_empty());
    *rig.fake.state.lock().unwrap() = None;
    calls
        .call(ops::SESSION_NEW, &args, &from_home())
        .expect("still runs");
    assert!(!rig.roots.is_live(JOB), "no cluster, nothing to trust");
}

#[test]
fn other_ops_pass_through_untouched() {
    let rig = rig();
    let inner = Arc::new(FakeCalls::default());
    let calls = UseCalls::new(inner.clone(), rig.uses.clone());
    calls
        .call("sessions.list", &json!({"job": JOB}), &from_home())
        .expect("passed");
    assert_eq!(inner.calls.lock().unwrap().len(), 1);
    assert!(!rig.roots.is_live(JOB));
}

#[cfg(unix)]
#[test]
fn a_job_known_only_by_a_start_is_ended_by_the_state_when_the_state_says_so() {
    let rig = rig();
    assert!(rig.roots.begin(JOB, StartTool::Claude));
    put(&rig, JOB, "api-key", "V", "NL-MARKER-1");
    let janitor = janitor(&rig);
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"));
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Done)]));
    janitor.tick().expect("swept");
    assert!(!has(&rig, JOB, "api-key"));
    assert!(rig.roots.is_ended(JOB));
}
