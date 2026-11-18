use super::*;
use notchling_engine::tools::agy_sidecar::agentapi::ApiError;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const ID: &str = "457369a9-35a1-40f8-92b9-26b6398b78a1";
const NEW: &str = "5ce2bd47-0c1e-4e6b-8f43-6aa0e2f1d3aa";
const FAST: Timing = Timing {
    polls: 3,
    poll: Duration::from_millis(0),
};

struct NoPause;

impl Pause for NoPause {
    fn pause(&self, _: Duration) {}
}

#[derive(Default)]
struct FakeApi {
    calls: Mutex<Vec<Vec<String>>>,
    answers: Mutex<VecDeque<Result<String, ApiError>>>,
}

impl FakeApi {
    fn answering(answers: Vec<Result<String, ApiError>>) -> FakeApi {
        FakeApi {
            calls: Mutex::new(Vec::new()),
            answers: Mutex::new(answers.into()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl AgentApi for FakeApi {
    fn run(&self, args: &[String]) -> Result<String, ApiError> {
        self.calls.lock().unwrap().push(args.to_vec());
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(ApiError::Missing))
    }
}

struct Counts(Mutex<VecDeque<Option<u64>>>, Mutex<Option<u64>>);

impl Counts {
    fn of(reads: &[Option<u64>]) -> Counts {
        Counts(
            Mutex::new(reads.iter().copied().collect()),
            Mutex::new(None),
        )
    }
}

impl Steps for Counts {
    fn exists(&self, _: &str) -> bool {
        self.count("").is_some()
    }

    fn count(&self, _: &str) -> Option<u64> {
        let mut reads = self.0.lock().unwrap();
        let mut last = self.1.lock().unwrap();
        match reads.pop_front() {
            Some(read) => {
                *last = read;
                read
            }
            None => *last,
        }
    }
}

fn sent() -> Result<String, ApiError> {
    Ok(r#"{"response":{"sendMessage":{}}}"#.into())
}

fn made() -> Result<String, ApiError> {
    Ok(format!(
        r#"{{"response":{{"newConversation":{{"conversationId":"{NEW}"}}}}}}"#
    ))
}

fn reply(id: &str, session: &str) -> Job {
    Job {
        id: id.into(),
        kind: Kind::Reply {
            session: session.into(),
        },
        text: "hello".into(),
    }
}

fn start(id: &str, model: Option<&str>) -> Job {
    Job {
        id: id.into(),
        kind: Kind::Start {
            model: model.map(String::from),
            title: Some("A title".into()),
        },
        text: "build it".into(),
    }
}

#[test]
fn a_reply_counts_as_delivered_only_when_the_steps_grew() {
    let api = FakeApi::answering(vec![sent()]);
    let steps = Counts::of(&[Some(4), Some(4), Some(6)]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    assert_eq!(
        execute(&reply("r1", ID), &work),
        Outcome::Delivered {
            session: Some(ID.into())
        }
    );
    assert_eq!(
        api.calls(),
        vec![vec!["send-message".to_string(), ID.into(), "hello".into()]]
    );
}

#[test]
fn a_message_agentapi_took_but_the_steps_never_show_is_unconfirmed_not_lost() {
    let api = FakeApi::answering(vec![sent()]);
    let steps = Counts::of(&[Some(4)]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    assert_eq!(
        execute(&reply("r1", ID), &work),
        Outcome::Unconfirmed {
            session: Some(ID.into())
        }
    );
}

#[test]
fn a_conversation_that_does_not_exist_fails_before_agentapi_would_silently_drop_it() {
    let api = FakeApi::answering(vec![sent()]);
    let steps = Counts::of(&[None]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    match execute(&reply("r1", ID), &work) {
        Outcome::Failed(why) => assert!(why.contains("no conversation"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(api.calls().is_empty(), "agentapi must not run");
}

#[test]
fn an_id_that_is_not_a_uuid_never_reaches_agentapi() {
    let api = FakeApi::answering(vec![sent()]);
    let steps = Counts::of(&[Some(1)]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    for bad in [
        "../x",
        "agy:457369a9",
        "--flag",
        "",
        "457369a9-35a1-40f8-92b9-26b6398b78a1 x",
    ] {
        assert!(
            matches!(execute(&reply("r", bad), &work), Outcome::Failed(_)),
            "{bad}"
        );
    }
    assert!(api.calls().is_empty());
}

#[test]
fn an_agentapi_that_never_started_is_a_visible_failure_with_its_words() {
    let api = FakeApi::answering(vec![Err(ApiError::Missing)]);
    let steps = Counts::of(&[Some(1)]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    assert_eq!(
        execute(&reply("r1", ID), &work),
        Outcome::Failed("agentapi was not found".into())
    );
}

#[test]
fn a_new_conversation_reports_its_id_and_passes_only_a_plain_model_name() {
    let api = FakeApi::answering(vec![made()]);
    let steps = Counts::of(&[Some(2)]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    assert_eq!(
        execute(&start("s1", Some("flash")), &work),
        Outcome::Delivered {
            session: Some(NEW.into())
        }
    );
    let call = &api.calls()[0];
    assert_eq!(call[0], "new-conversation");
    assert!(call.contains(&"--model=flash".to_string()));
    assert_eq!(
        &call[call.len() - 2..],
        ["--".to_string(), "build it".into()]
    );

    let api = FakeApi::answering(vec![made()]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    assert!(matches!(
        execute(&start("s2", Some("--dangerous")), &work),
        Outcome::Failed(_)
    ));
    assert!(api.calls().is_empty());
}

#[derive(Default)]
struct Hub {
    takes: VecDeque<Result<Value, LinkError>>,
    take_args: Vec<Value>,
    reports: Vec<(u32, Value)>,
    report_failures: u32,
    report_refused: bool,
    connects: u32,
    connect_fails: VecDeque<bool>,
    take_on: Vec<u32>,
}

type Shared = Arc<Mutex<Hub>>;

struct HubLink {
    hub: Shared,
    n: u32,
}

impl Link for HubLink {
    fn call(&mut self, op: &str, args: Value, _: Duration) -> Result<Value, LinkError> {
        let mut hub = self.hub.lock().unwrap();
        if op == ops::AGY_SIDECAR_REPORT {
            if hub.report_refused {
                return Err(LinkError::Refused("no".into()));
            }
            if hub.report_failures > 0 {
                hub.report_failures -= 1;
                return Err(LinkError::Gone);
            }
            hub.reports.push((self.n, args));
            return Ok(json!({"accepted": true}));
        }
        assert_eq!(op, ops::AGY_SIDECAR_TAKE);
        hub.take_args.push(args);
        hub.take_on.push(self.n);
        hub.takes.pop_front().unwrap_or(Err(LinkError::Gone))
    }
}

struct HubConnect(Shared);

impl Connect for HubConnect {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError> {
        let mut hub = self.0.lock().unwrap();
        hub.connects += 1;
        let n = hub.connects;
        if hub.connect_fails.pop_front().unwrap_or(false) {
            return Err(LinkError::Gone);
        }
        Ok(Box::new(HubLink {
            hub: self.0.clone(),
            n,
        }))
    }
}

fn hub(takes: Vec<Result<Value, LinkError>>) -> (Shared, HubConnect) {
    let shared: Shared = Arc::new(Mutex::new(Hub {
        takes: takes.into(),
        ..Hub::default()
    }));
    (shared.clone(), HubConnect(shared))
}

#[derive(Default)]
struct Rests(Vec<u32>);

impl Rest for Rests {
    fn rest(&mut self, attempt: u32) {
        self.0.push(attempt);
    }
}

fn working<'a>(api: &'a FakeApi, steps: &'a Counts) -> Work<'a> {
    Work {
        api,
        steps,
        pause: &NoPause,
        timing: FAST,
    }
}

#[test]
fn every_taken_job_is_reported_with_its_own_id_and_each_take_asks_for_one_job() {
    let api = FakeApi::answering(vec![sent(), sent()]);
    let steps = Counts::of(&[Some(1), Some(2), Some(2), Some(3)]);
    let work = working(&api, &steps);
    let (shared, mut connect) = hub(vec![Ok(json!({"jobs": [
        reply("a", ID).to_json(),
        reply("b", ID).to_json(),
        {"kind": "nope"},
    ]}))]);
    let mut rests = Rests::default();
    assert_eq!(serve_once(&mut connect, &mut rests, &work), Ok(2));
    let hub = shared.lock().unwrap();
    let ids: Vec<&str> = hub
        .reports
        .iter()
        .map(|(_, r)| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(hub.reports[0].1["result"], "delivered");
    assert_eq!(hub.take_args[0]["max"], 1);
}

#[test]
fn a_result_goes_back_on_a_fresh_connection_because_the_one_that_took_the_job_sat_idle() {
    let api = FakeApi::answering(vec![sent()]);
    let steps = Counts::of(&[Some(1), Some(2)]);
    let work = working(&api, &steps);
    let (shared, mut connect) = hub(vec![Ok(json!({"jobs": [reply("a", ID).to_json()]}))]);
    let mut rests = Rests::default();
    serve_once(&mut connect, &mut rests, &work).unwrap();
    let hub = shared.lock().unwrap();
    assert_eq!(hub.take_on, vec![1]);
    assert_eq!(hub.reports.len(), 1);
    assert_ne!(hub.reports[0].0, 1);
}

#[test]
fn a_report_that_fails_is_retried_until_it_lands_and_the_outcome_is_not_lost() {
    let api = FakeApi::answering(vec![sent()]);
    let steps = Counts::of(&[Some(1), Some(2)]);
    let work = working(&api, &steps);
    let (shared, mut connect) = hub(vec![Ok(json!({"jobs": [reply("a", ID).to_json()]}))]);
    {
        let mut hub = shared.lock().unwrap();
        hub.report_failures = 3;
        hub.connect_fails = VecDeque::from([false, true]);
    }
    let mut rests = Rests::default();
    assert_eq!(serve_once(&mut connect, &mut rests, &work), Ok(1));
    let hub = shared.lock().unwrap();
    assert_eq!(hub.reports.len(), 1);
    assert_eq!(hub.reports[0].1["id"], "a");
    assert_eq!(rests.0, vec![1, 2, 3, 4]);
}

#[test]
fn a_report_the_daemon_refuses_is_not_retried_and_one_for_a_gone_daemon_gives_up() {
    let api = FakeApi::answering(vec![sent(), sent()]);
    let steps = Counts::of(&[Some(1), Some(2), Some(2), Some(3)]);
    let work = working(&api, &steps);
    let (shared, mut connect) = hub(vec![Ok(json!({"jobs": [reply("a", ID).to_json()]}))]);
    shared.lock().unwrap().report_refused = true;
    let mut rests = Rests::default();
    assert_eq!(serve_once(&mut connect, &mut rests, &work), Ok(1));
    assert!(rests.0.is_empty());
    assert_eq!(shared.lock().unwrap().connects, 2);

    let (shared, mut connect) = hub(vec![Ok(json!({"jobs": [reply("b", ID).to_json()]}))]);
    shared.lock().unwrap().report_failures = 1000;
    let mut rests = Rests::default();
    assert_eq!(serve_once(&mut connect, &mut rests, &work), Ok(1));
    assert_eq!(rests.0, (1..=RETRIES).collect::<Vec<_>>());
}

#[test]
fn a_batch_is_never_dropped_when_one_report_cannot_be_delivered() {
    let api = FakeApi::answering(vec![sent(), sent()]);
    let steps = Counts::of(&[Some(1), Some(2), Some(2), Some(3)]);
    let work = working(&api, &steps);
    let (shared, mut connect) = hub(vec![Ok(json!({"jobs": [
        reply("a", ID).to_json(),
        reply("b", ID).to_json(),
    ]}))]);
    shared.lock().unwrap().report_failures = 1;
    let mut rests = Rests::default();
    assert_eq!(serve_once(&mut connect, &mut rests, &work), Ok(2));
    let hub = shared.lock().unwrap();
    let ids: Vec<&str> = hub
        .reports
        .iter()
        .map(|(_, r)| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(api.calls().len(), 2);
}

#[test]
fn it_reconnects_when_the_daemon_comes_back() {
    let api = FakeApi::default();
    let steps = Counts::of(&[]);
    let work = working(&api, &steps);
    let (shared, mut connect) = hub(vec![
        Ok(json!({"jobs": []})),
        Err(LinkError::Gone),
        Ok(json!({"jobs": []})),
    ]);
    shared.lock().unwrap().connect_fails = VecDeque::from([true, false, false, true, false]);
    let mut rests = Rests::default();
    serve(&mut connect, &mut rests, &work, &|| false);
    assert!(shared.lock().unwrap().connects >= 5);
    assert!(shared.lock().unwrap().takes.is_empty());
}

#[test]
fn it_exits_cleanly_when_the_daemon_stays_gone() {
    let api = FakeApi::default();
    let steps = Counts::of(&[]);
    let work = working(&api, &steps);
    let (shared, mut connect) = hub(Vec::new());
    shared.lock().unwrap().connect_fails = VecDeque::from(vec![true; 100]);
    let mut rests = Rests::default();
    serve(&mut connect, &mut rests, &work, &|| false);
    assert_eq!(shared.lock().unwrap().connects, RETRIES + 1);
    assert_eq!(rests.0, (1..=RETRIES).collect::<Vec<_>>());
}

#[test]
fn it_stops_at_once_when_asked() {
    let api = FakeApi::default();
    let steps = Counts::of(&[]);
    let work = working(&api, &steps);
    let (shared, mut connect) = hub(Vec::new());
    let mut rests = Rests::default();
    serve(&mut connect, &mut rests, &work, &|| true);
    assert_eq!(shared.lock().unwrap().connects, 0);
}

#[test]
fn a_timeout_or_a_bad_exit_after_agentapi_started_is_reported_unconfirmed_not_failed() {
    for error in [
        ApiError::TimedOut,
        ApiError::Failed("agentapi: boom".into()),
    ] {
        let api = FakeApi::answering(vec![Err(error)]);
        let steps = Counts::of(&[Some(1)]);
        assert_eq!(
            execute(&reply("r1", ID), &working(&api, &steps)),
            Outcome::Unconfirmed {
                session: Some(ID.into())
            }
        );
    }
    let api = FakeApi::answering(vec![Err(ApiError::TimedOut)]);
    let steps = Counts::of(&[]);
    assert_eq!(
        execute(&start("s1", None), &working(&api, &steps)),
        Outcome::Unconfirmed { session: None }
    );
    let api = FakeApi::answering(vec![Err(ApiError::NotStarted(
        "agentapi could not start".into(),
    ))]);
    let steps = Counts::of(&[Some(1)]);
    assert!(matches!(
        execute(&reply("r1", ID), &working(&api, &steps)),
        Outcome::Failed(_)
    ));
}
