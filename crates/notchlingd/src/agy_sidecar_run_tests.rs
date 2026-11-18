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
fn a_failing_agentapi_is_a_visible_failure() {
    let api = FakeApi::answering(vec![Err(ApiError::Failed("boom".into()))]);
    let steps = Counts::of(&[Some(1)]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    assert_eq!(
        execute(&reply("r1", ID), &work),
        Outcome::Failed("boom".into())
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

struct Script {
    takes: VecDeque<Result<Value, LinkError>>,
    reports: Vec<Value>,
    report_fails: bool,
}

impl Link for Script {
    fn call(&mut self, op: &str, args: Value, _: Duration) -> Result<Value, LinkError> {
        if op == ops::AGY_SIDECAR_REPORT {
            if self.report_fails {
                return Err(LinkError::Gone);
            }
            self.reports.push(args);
            return Ok(json!({"accepted": true}));
        }
        assert_eq!(op, ops::AGY_SIDECAR_TAKE);
        self.takes.pop_front().unwrap_or(Err(LinkError::Gone))
    }
}

fn script(takes: Vec<Result<Value, LinkError>>) -> Script {
    Script {
        takes: takes.into(),
        reports: Vec::new(),
        report_fails: false,
    }
}

#[test]
fn every_taken_job_is_reported_with_its_own_id() {
    let api = FakeApi::answering(vec![sent(), sent()]);
    let steps = Counts::of(&[Some(1), Some(2), Some(2), Some(3)]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    let mut link = script(vec![Ok(json!({"jobs": [
        reply("a", ID).to_json(),
        reply("b", ID).to_json(),
        {"kind": "nope"},
    ]}))]);
    assert_eq!(serve_once(&mut link, &work), Ok(2));
    let ids: Vec<&str> = link
        .reports
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(link.reports[0]["result"], "delivered");
}

#[test]
fn a_report_that_cannot_be_sent_ends_the_connection_so_it_is_retried() {
    let api = FakeApi::answering(vec![sent()]);
    let steps = Counts::of(&[Some(1), Some(2)]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    let mut link = script(vec![Ok(json!({"jobs": [reply("a", ID).to_json()]}))]);
    link.report_fails = true;
    assert_eq!(serve_once(&mut link, &work), Err(LinkError::Gone));
}

struct Connects {
    results: VecDeque<Result<Script, LinkError>>,
    tried: u32,
}

impl Connect for Connects {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError> {
        self.tried += 1;
        match self.results.pop_front() {
            Some(Ok(link)) => Ok(Box::new(link)),
            Some(Err(e)) => Err(e),
            None => Err(LinkError::Gone),
        }
    }
}

#[derive(Default)]
struct Rests(Vec<u32>);

impl Rest for Rests {
    fn rest(&mut self, attempt: u32) {
        self.0.push(attempt);
    }
}

#[test]
fn it_reconnects_when_the_daemon_comes_back() {
    let api = FakeApi::default();
    let steps = Counts::of(&[]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    let mut connects = Connects {
        results: vec![
            Err(LinkError::Gone),
            Ok(script(vec![Ok(json!({"jobs": []})), Err(LinkError::Gone)])),
            Err(LinkError::Gone),
            Ok(script(vec![Ok(json!({"jobs": []}))])),
        ]
        .into(),
        tried: 0,
    };
    let mut rests = Rests::default();
    serve(&mut connects, &mut rests, &work, &|| false);
    assert!(connects.tried >= 4);
    assert!(connects.results.is_empty());
}

#[test]
fn it_exits_cleanly_when_the_daemon_stays_gone() {
    let api = FakeApi::default();
    let steps = Counts::of(&[]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    let mut connects = Connects {
        results: VecDeque::new(),
        tried: 0,
    };
    let mut rests = Rests::default();
    serve(&mut connects, &mut rests, &work, &|| false);
    assert_eq!(connects.tried, RETRIES + 1);
    assert_eq!(rests.0, (1..=RETRIES).collect::<Vec<_>>());
}

#[test]
fn it_stops_at_once_when_asked() {
    let api = FakeApi::default();
    let steps = Counts::of(&[]);
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &NoPause,
        timing: FAST,
    };
    let mut connects = Connects {
        results: VecDeque::new(),
        tried: 0,
    };
    let mut rests = Rests::default();
    let stopped = Arc::new(Mutex::new(true));
    let flag = stopped.clone();
    serve(&mut connects, &mut rests, &work, &move || {
        *flag.lock().unwrap()
    });
    assert_eq!(connects.tried, 0);
}
