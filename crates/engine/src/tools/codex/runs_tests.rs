use super::super::fake::{FakeRun, FakeSpawner};
use super::*;
use std::time::Instant;

#[derive(Default)]
struct Collect(Mutex<Vec<TurnEvent>>);

impl TurnSink for Collect {
    fn event(&self, event: TurnEvent) {
        self.0.lock().unwrap().push(event);
    }
}

impl Collect {
    fn all(&self) -> Vec<TurnEvent> {
        self.0.lock().unwrap().clone()
    }

    /// Waits until `n` events are in.
    fn wait_for(&self, n: usize) -> Vec<TurnEvent> {
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            let all = self.all();
            if all.len() >= n {
                return all;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("waited for {n} events, got {:?}", self.all());
    }
}

fn setup() -> (CodexRuns, Arc<FakeSpawner>, Arc<Collect>) {
    let spawner = Arc::new(FakeSpawner::new());
    let sink = Arc::new(Collect::default());
    let runs = CodexRuns::new(
        spawner.clone(),
        PathBuf::from("/bin/codex"),
        Some("/usr/bin".into()),
        sink.clone(),
    );
    (runs, spawner, sink)
}

fn request(text: &str, session: Option<&str>) -> Request {
    Request {
        text: text.into(),
        session: session.map(String::from),
        folder: "/work".into(),
        pick: Pick::default(),
    }
}

const STARTED: &str = r#"{"type":"thread.started","thread_id":"sess-1"}"#;
const DONE: &str = r#"{"type":"turn.completed","usage":{}}"#;

fn ended(session: &str, failure: Option<&str>) -> TurnEvent {
    TurnEvent::Ended {
        session: Some(session.into()),
        failure: failure.map(String::from),
    }
}

fn started(session: &str) -> TurnEvent {
    TurnEvent::Started {
        session: session.into(),
    }
}

fn idle(runs: &CodexRuns) {
    let until = Instant::now() + Duration::from_secs(10);
    while runs.active() > 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(runs.active(), 0);
}

#[test]
fn a_new_session_is_named_by_thread_started() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::ok(&[STARTED, DONE]));
    let sent = runs.send(request("hello", None)).expect("sends");
    assert_eq!(
        sent,
        Sent::Started {
            session: "sess-1".into()
        }
    );
    assert_eq!(sink.wait_for(2), [started("sess-1"), ended("sess-1", None)]);
    idle(&runs);
    let spec = &spawner.specs()[0];
    assert_eq!(spec.program, PathBuf::from("/bin/codex"));
    assert_eq!(spec.cwd, "/work");
    assert_eq!(&spec.args[spec.args.len() - 3..], ["-C", "/work", "hello"]);
    assert_eq!(spec.args[0], "exec");
    assert!(runs.running().is_empty());
}

#[test]
fn the_next_turn_of_a_session_resumes_it() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::ok(&[DONE]));
    let sent = runs.send(request("more", Some("abc"))).expect("sends");
    assert_eq!(
        sent,
        Sent::Started {
            session: "abc".into()
        }
    );
    assert_eq!(sink.wait_for(2), [started("abc"), ended("abc", None)]);
    let args = &spawner.specs()[0].args;
    assert_eq!(&args[..2], ["exec", "resume"]);
    assert_eq!(&args[args.len() - 2..], ["abc", "more"]);
}

#[test]
fn a_message_sent_mid_turn_waits_and_goes_out_next() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::held(&[DONE]));
    spawner.push(FakeRun::ok(&[DONE]));
    runs.send(request("first", Some("abc"))).expect("sends");
    assert!(runs.is_running("abc"));
    let queued = runs.send(request("second", Some("abc"))).expect("queues");
    assert_eq!(
        queued,
        Sent::Queued {
            session: "abc".into(),
            waiting: 1
        }
    );
    assert_eq!(runs.waiting("abc"), 1);
    assert_eq!(spawner.specs().len(), 1, "one turn at a time");
    spawner.processes()[0].release();
    assert_eq!(
        sink.wait_for(5),
        [
            started("abc"),
            TurnEvent::Queued {
                session: "abc".into(),
                waiting: 1
            },
            ended("abc", None),
            started("abc"),
            ended("abc", None),
        ]
    );
    idle(&runs);
    let specs = spawner.specs();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[1].args.last().map(String::as_str), Some("second"));
    assert_eq!(runs.waiting("abc"), 0);
}

#[test]
fn stop_interrupts_and_a_stop_is_not_a_failure() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::held(&[]));
    runs.send(request("go", Some("abc"))).expect("sends");
    assert!(runs.stop("abc"));
    assert_eq!(spawner.processes()[0].interrupts(), 1);
    assert_eq!(sink.wait_for(2), [started("abc"), ended("abc", None)]);
    assert!(!runs.stop("abc"), "nothing running now");
    assert!(!runs.stop("other"));
}

#[test]
fn stop_leaves_the_waiting_messages_to_go_out() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::held(&[]));
    spawner.push(FakeRun::ok(&[DONE]));
    runs.send(request("first", Some("abc"))).expect("sends");
    runs.send(request("second", Some("abc"))).expect("queues");
    runs.stop("abc");
    sink.wait_for(5);
    idle(&runs);
    assert_eq!(spawner.specs().len(), 2);
}

#[test]
fn forget_drops_the_waiting_messages_too() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::held(&[]));
    runs.send(request("first", Some("abc"))).expect("sends");
    runs.send(request("second", Some("abc"))).expect("queues");
    assert!(runs.forget("abc"));
    sink.wait_for(3);
    idle(&runs);
    assert_eq!(spawner.specs().len(), 1);
}

#[test]
fn a_failed_turn_reports_what_it_printed_last_and_the_queue_goes_on() {
    let (runs, spawner, sink) = setup();
    spawner
        .push(FakeRun::held(&[r#"{"type":"error","message":"stream disconnected"}"#]).exiting(1));
    spawner.push(FakeRun::ok(&[DONE]));
    runs.send(request("first", Some("abc"))).expect("sends");
    runs.send(request("second", Some("abc"))).expect("queues");
    spawner.processes()[0].release();
    let events = sink.wait_for(5);
    assert!(
        events.contains(&ended("abc", Some("stream disconnected"))),
        "{events:?}"
    );
    idle(&runs);
    assert_eq!(spawner.specs().len(), 2, "the next message still goes out");
}

#[test]
fn an_error_on_standard_error_is_the_failure_when_nothing_better_was_printed() {
    let (runs, spawner, sink) = setup();
    spawner.push(
        FakeRun::ok(&[])
            .stderr("warning: fine")
            .stderr("Error: no such model")
            .exiting(2),
    );
    runs.send(request("go", Some("abc"))).expect("sends");
    assert_eq!(
        sink.wait_for(2),
        [started("abc"), ended("abc", Some("Error: no such model"))]
    );
}

#[test]
fn a_new_session_that_is_never_named_is_an_error() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::ok(&[]).exiting(1).stderr("Error: not logged in"));
    let got = runs.send(request("go", None));
    assert_eq!(
        got,
        Err(SendError::NotStarted("Error: not logged in".into()))
    );
    assert_eq!(
        sink.wait_for(1),
        [TurnEvent::Ended {
            session: None,
            failure: Some("Error: not logged in".into())
        }]
    );
    idle(&runs);

    spawner.push(FakeRun::ok(&[]));
    assert_eq!(
        runs.send(request("go", None)),
        Err(SendError::NotStarted(
            "Codex didn't start a session.".into()
        ))
    );
    idle(&runs);
}

#[test]
fn a_session_that_takes_too_long_to_be_named_is_stopped() {
    let (runs, spawner, _sink) = setup();
    runs.set_announce_wait(Duration::from_millis(50));
    spawner.push(FakeRun::held(&[]));
    let got = runs.send(request("go", None));
    assert!(matches!(got, Err(SendError::NotStarted(_))), "{got:?}");
    assert_eq!(spawner.processes()[0].interrupts(), 1);
    idle(&runs);
}

#[test]
fn a_spawn_error_frees_the_slot() {
    let (runs, spawner, sink) = setup();
    spawner.push_error("no such file");
    assert_eq!(
        runs.send(request("go", Some("abc"))),
        Err(SendError::Spawn("couldn't start it: no such file".into()))
    );
    assert_eq!(runs.active(), 0);
    assert!(!runs.is_running("abc"));
    spawner.push_error("no such file");
    assert!(runs.send(request("go", None)).is_err());
    assert_eq!(runs.active(), 0);
    assert!(sink.all().is_empty());
}

#[test]
fn a_waiting_message_that_cannot_start_is_reported_and_the_rest_dropped() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::held(&[]));
    runs.send(request("first", Some("abc"))).expect("sends");
    runs.send(request("second", Some("abc"))).expect("queues");
    runs.send(request("third", Some("abc"))).expect("queues");
    spawner.push_error("gone");
    spawner.processes()[0].release();
    let events = sink.wait_for(5);
    let last = events.last().cloned();
    assert_eq!(
        last,
        Some(ended(
            "abc",
            Some("couldn't start the next message: couldn't start it: gone (1 more waiting messages were dropped)")
        )),
        "{events:?}"
    );
    idle(&runs);
    assert!(!runs.is_running("abc"));
    assert_eq!(runs.waiting("abc"), 0);
}

#[test]
fn only_so_many_turns_run_at_once() {
    let (runs, spawner, _sink) = setup();
    for n in 0..MAX_RUNNING {
        spawner.push(FakeRun::held(&[]));
        runs.send(request("go", Some(&format!("s{n}"))))
            .expect("sends");
    }
    assert_eq!(
        runs.send(request("go", Some("extra"))),
        Err(SendError::TooMany)
    );
    // A message for a session that is running still queues: it takes no slot.
    assert!(matches!(
        runs.send(request("more", Some("s0"))),
        Ok(Sent::Queued { .. })
    ));
    for process in spawner.processes() {
        process.release();
    }
    idle(&runs);
}

#[test]
fn network_and_picks_reach_the_command() {
    let (runs, spawner, _sink) = setup();
    runs.set_network(false);
    spawner.push(FakeRun::ok(&[]));
    let mut r = request("go", Some("abc"));
    r.pick = Pick {
        model: Some("m1".into()),
        effort: Some("high".into()),
    };
    runs.send(r).expect("sends");
    idle(&runs);
    let args = &spawner.specs()[0].args;
    assert!(!args.iter().any(|a| a.contains("network_access")));
    assert!(args.windows(2).any(|w| w == ["-m", "m1"]));
    assert!(args.iter().any(|a| a == "model_reasoning_effort=\"high\""));
}

#[test]
fn a_waiting_message_without_a_folder_runs_where_the_session_does() {
    let (runs, spawner, sink) = setup();
    spawner.push(FakeRun::held(&[]));
    spawner.push(FakeRun::ok(&[]));
    runs.send(request("first", Some("abc"))).expect("sends");
    let mut next = request("second", Some("abc"));
    next.folder = String::new();
    runs.send(next).expect("queues");
    spawner.processes()[0].release();
    sink.wait_for(5);
    idle(&runs);
    assert_eq!(spawner.specs()[1].cwd, "/work");
}
