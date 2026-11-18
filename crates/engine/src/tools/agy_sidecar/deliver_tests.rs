use super::agentapi::{AgentApi, ApiError};
use super::deliver::{send, start, Fail, Pause, Sent, Started, Timing};
use super::steps::Steps;
use std::sync::Mutex;
use std::time::Duration;

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
    answer: Mutex<Option<Result<String, ApiError>>>,
}

impl FakeApi {
    fn answering(answer: Result<String, ApiError>) -> FakeApi {
        FakeApi {
            calls: Mutex::new(Vec::new()),
            answer: Mutex::new(Some(answer)),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl AgentApi for FakeApi {
    fn run(&self, args: &[String]) -> Result<String, ApiError> {
        self.calls.lock().unwrap().push(args.to_vec());
        self.answer
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Err(ApiError::Missing))
    }
}

struct Counts {
    reads: Mutex<Vec<Option<u64>>>,
    last: Mutex<Option<u64>>,
}

impl Counts {
    fn of(reads: &[Option<u64>]) -> Counts {
        Counts {
            reads: Mutex::new(reads.iter().rev().copied().collect()),
            last: Mutex::new(None),
        }
    }
}

impl Steps for Counts {
    fn exists(&self, _: &str) -> bool {
        self.last.lock().unwrap().is_some()
    }

    fn count(&self, _: &str) -> Option<u64> {
        let mut reads = self.reads.lock().unwrap();
        let next = match reads.pop() {
            Some(read) => read,
            None => *self.last.lock().unwrap(),
        };
        *self.last.lock().unwrap() = next;
        next
    }
}

fn sent_ok() -> Result<String, ApiError> {
    Ok(r#"{"response":{"sendMessage":{}}}"#.to_string())
}

#[test]
fn a_message_counts_as_delivered_when_the_steps_grow() {
    let api = FakeApi::answering(sent_ok());
    let steps = Counts::of(&[Some(4), Some(4), Some(5)]);
    let out = send(&api, &steps, &NoPause, ID, "hello", FAST).unwrap();
    assert_eq!(
        out,
        Sent::Confirmed {
            before: 4,
            after: 5
        }
    );
    assert_eq!(
        api.calls(),
        vec![vec!["send-message".to_string(), ID.into(), "hello".into()]]
    );
}

#[test]
fn a_wrong_id_never_reaches_agentapi_because_agentapi_would_drop_it_silently() {
    let api = FakeApi::answering(sent_ok());
    let steps = Counts::of(&[None]);
    assert_eq!(
        send(&api, &steps, &NoPause, ID, "hello", FAST),
        Err(Fail::NoConversation)
    );
    assert!(api.calls().is_empty());
}

#[test]
fn an_id_that_is_not_a_uuid_is_refused_before_anything_runs() {
    let api = FakeApi::answering(sent_ok());
    let steps = Counts::of(&[Some(1)]);
    for bad in [
        "",
        "--help",
        "../x",
        "agy:457369a9-35a1-40f8-92b9-26b6398b78a1",
    ] {
        assert_eq!(
            send(&api, &steps, &NoPause, bad, "hello", FAST),
            Err(Fail::BadId),
            "{bad}"
        );
    }
    assert!(api.calls().is_empty());
}

#[test]
fn a_message_agentapi_took_but_no_step_shows_for_is_unconfirmed_not_lost() {
    let api = FakeApi::answering(sent_ok());
    let steps = Counts::of(&[Some(7)]);
    assert_eq!(
        send(&api, &steps, &NoPause, ID, "hello", FAST),
        Ok(Sent::Unconfirmed { before: 7 })
    );
}

#[test]
fn a_failing_agentapi_is_a_visible_failure() {
    let api = FakeApi::answering(Err(ApiError::Failed("agentapi: boom".into())));
    let steps = Counts::of(&[Some(1)]);
    assert_eq!(
        send(&api, &steps, &NoPause, ID, "hello", FAST),
        Err(Fail::Api("agentapi: boom".into()))
    );
    let api = FakeApi::answering(Ok("{}".into()));
    assert!(matches!(
        send(&api, &steps, &NoPause, ID, "hello", FAST),
        Err(Fail::Api(_))
    ));
}

#[test]
fn text_that_looks_like_a_flag_is_passed_as_text() {
    let api = FakeApi::answering(sent_ok());
    let steps = Counts::of(&[Some(1), Some(2)]);
    send(&api, &steps, &NoPause, ID, "--help; rm -rf /", FAST).unwrap();
    assert_eq!(api.calls()[0][2], "--help; rm -rf /");
    let api = FakeApi::answering(Ok(format!(
        r#"{{"response":{{"newConversation":{{"conversationId":"{NEW}"}}}}}}"#
    )));
    let steps = Counts::of(&[Some(1)]);
    start(&api, &steps, &NoPause, "-x do it", None, None, FAST).unwrap();
    assert_eq!(
        api.calls()[0],
        vec![
            "new-conversation".to_string(),
            "--".into(),
            "-x do it".into()
        ]
    );
}

#[test]
fn a_new_conversation_is_confirmed_by_its_database_and_carries_model_and_title() {
    let api = FakeApi::answering(Ok(format!(
        r#"{{"response":{{"newConversation":{{"prompt":"p","conversationId":"{NEW}"}}}}}}"#
    )));
    let steps = Counts::of(&[None, Some(2)]);
    let out = start(
        &api,
        &steps,
        &NoPause,
        "do it",
        Some("flash"),
        Some("A\nB"),
        FAST,
    )
    .unwrap();
    assert_eq!(out, Started::Confirmed(NEW.into()));
    assert_eq!(
        api.calls()[0],
        vec![
            "new-conversation".to_string(),
            "--model=flash".into(),
            "--title=A B".into(),
            "--".into(),
            "do it".into()
        ]
    );
}

#[test]
fn a_new_conversation_with_no_database_yet_is_unconfirmed_and_keeps_its_id() {
    let api = FakeApi::answering(Ok(format!(
        r#"{{"response":{{"newConversation":{{"conversationId":"{NEW}"}}}}}}"#
    )));
    let steps = Counts::of(&[None]);
    assert_eq!(
        start(&api, &steps, &NoPause, "x", None, None, FAST),
        Ok(Started::Unconfirmed(NEW.into()))
    );
}

#[test]
fn a_model_that_is_not_a_plain_name_is_refused_and_a_bad_id_back_is_refused() {
    let api = FakeApi::answering(sent_ok());
    let steps = Counts::of(&[None]);
    assert!(matches!(
        start(&api, &steps, &NoPause, "x", Some("--rm"), None, FAST),
        Err(Fail::Refused(_))
    ));
    assert!(api.calls().is_empty());
    let api = FakeApi::answering(Ok(
        r#"{"response":{"newConversation":{"conversationId":"../../x"}}}"#.into(),
    ));
    assert!(matches!(
        start(&api, &steps, &NoPause, "x", None, None, FAST),
        Err(Fail::Api(_))
    ));
}
