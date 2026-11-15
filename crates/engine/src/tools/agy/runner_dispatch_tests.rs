use super::*;
use crate::tools::acp::fake::Responder;

fn holding_load(load: Arc<Mutex<Option<Value>>>) -> FakeSpawner {
    let responder: Responder = Arc::new(move |m, p| match m["method"].as_str() {
        Some("initialize") => ok(p, m, json!({"protocolVersion": 1})),
        Some("session/load") => *load.lock().unwrap() = Some(m.clone()),
        _ => {}
    });
    FakeSpawner::with_responder(responder)
}

fn wait_for_load(load: &Arc<Mutex<Option<Value>>>) -> Value {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(request) = load.lock().unwrap().clone() {
            return request;
        }
        assert!(Instant::now() < end, "session/load never came");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn dispatch_returns_only_once_the_prompt_is_written() {
    let load = Arc::new(Mutex::new(None));
    let rig = rig_with(holding_load(load.clone()), Some("/fake/agy"));
    let runner = rig.runner.clone();
    let sending =
        std::thread::spawn(move || runner.dispatch("x", "hello", "/work", Duration::from_secs(10)));
    let request = wait_for_load(&load);
    std::thread::sleep(Duration::from_millis(100));
    assert!(!sending.is_finished(), "still waiting on the load");
    let process = rig.spawner.spawned().remove(0);
    assert!(process.requests("session/prompt").is_empty());
    ok(&process, &request, json!({}));
    assert_eq!(sending.join().unwrap(), Ok(()));
    assert_eq!(process.requests("session/prompt").len(), 1);
}

#[test]
fn dispatch_reports_a_failed_load() {
    let load = Arc::new(Mutex::new(None));
    let rig = rig_with(holding_load(load.clone()), Some("/fake/agy"));
    let runner = rig.runner.clone();
    let sending =
        std::thread::spawn(move || runner.dispatch("x", "hello", "/work", Duration::from_secs(10)));
    let request = wait_for_load(&load);
    let process = rig.spawner.spawned().remove(0);
    process.push(&json!({"jsonrpc": "2.0", "id": request["id"],
                         "error": {"code": -1, "message": "no such conversation"}}));
    assert_eq!(
        sending.join().unwrap(),
        Err(RunError::Start("no such conversation".into()))
    );
    assert!(process.requests("session/prompt").is_empty());
    assert!(!rig.runner.is_working("x"));
}

#[test]
fn dispatch_still_queued_at_the_bound_is_pending() {
    let load = Arc::new(Mutex::new(None));
    let rig = rig_with(holding_load(load.clone()), Some("/fake/agy"));
    let got = rig
        .runner
        .dispatch("x", "hello", "/work", Duration::from_millis(100));
    assert_eq!(got, Err(RunError::Pending));
    let process = rig.spawner.spawned().remove(0);
    assert!(process.requests("session/prompt").is_empty());
}

#[test]
fn dispatch_after_starting_went_off_sends_nothing() {
    let rig = rig();
    loaded(&rig);
    let on = Arc::new(AtomicBool::new(false));
    let flag = on.clone();
    rig.runner
        .set_may_start(Arc::new(move || flag.load(Ordering::SeqCst)));
    let got = rig
        .runner
        .dispatch(SESSION, "hello", "/work", Duration::from_secs(10));
    assert_eq!(got, Err(RunError::NotAllowed));
    assert!(rig.spawner.spawned()[0]
        .requests("session/prompt")
        .is_empty());
    assert!(!rig.runner.is_working(SESSION));
}
