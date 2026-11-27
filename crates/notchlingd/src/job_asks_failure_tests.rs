use super::tests::{args_for, job, member, rig};
use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{agent_caller, verified};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::ConsentStore;
use notchling_events::{MemorySink, Salt};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Duration;

const SIGNED_OUT: &str =
    "Claude Code isn't signed in on build. Sign in there, then dispatch again.";

struct FailingHands {
    error: ApiError,
}

impl Hands for FailingHands {
    fn dispatch(&self, _args: &Value) -> Result<Value, ApiError> {
        Err(self.error.clone())
    }
    fn tail(&self, _job: &JobRow, _from: u64) -> Result<Value, ApiError> {
        Err(self.error.clone())
    }
    fn give_back(&self, _job: &JobRow) -> Result<Value, ApiError> {
        Err(self.error.clone())
    }
    fn halt(&self, _job: &JobRow) -> Result<Value, ApiError> {
        Err(self.error.clone())
    }
}

struct OneFleet;

impl Fleet for OneFleet {
    fn member(&self, name_or_id: &str) -> Option<Member> {
        let found = member("0123456789abcdef", "build", "Personal");
        (found.id == name_or_id || found.name == name_or_id).then_some(found)
    }
    fn jobs(&self) -> Vec<JobRow> {
        vec![job("j-own", "Personal")]
    }
    fn secret_organization(&self, _name: &str) -> Option<String> {
        None
    }
}

pub(super) struct Failing {
    _tmp: TempDir,
    pub(super) consent: Arc<ConsentOps>,
    pub(super) asks: Arc<JobAsks>,
    pub(super) inbox: Arc<InboxOps>,
}

impl Drop for Failing {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn failing(tag: &str, error: ApiError) -> Failing {
    failing_with(tag, error, Arc::new(JobBook::in_memory()))
}

pub(super) fn failing_with(tag: &str, error: ApiError, book: Arc<JobBook>) -> Failing {
    failing_hands(tag, Arc::new(FailingHands { error }), book)
}

pub(super) fn failing_hands(tag: &str, hands: Arc<dyn Hands>, book: Arc<JobBook>) -> Failing {
    let tmp = TempDir::new(&format!("job-asks-fail-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        Some(inbox.clone()),
        Some(Events::fixed(Arc::new(MemorySink::new()), Salt([3; 32]))),
        Arc::new(now_ms),
    );
    let mut me = verified("parent");
    me.organization = Some("Personal".to_string());
    let who = Arc::new(Mutex::new(me));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();
    let fleet = Arc::new(OneFleet);
    let on = Arc::new(AtomicBool::new(true));
    let asks = JobAsks::new(
        consent.clone(),
        fleet,
        hands,
        book,
        Arc::new(move || on.load(std::sync::atomic::Ordering::SeqCst)),
    );
    Failing {
        _tmp: tmp,
        consent,
        asks,
        inbox,
    }
}

pub(super) fn dispatch_failed(f: &Failing, key: &str) -> Consent {
    let reply = f
        .asks
        .call(
            &agent_caller(),
            ops::CLUSTER_DISPATCH,
            &args_for("claude", key),
        )
        .unwrap();
    let id = reply["request"]["id"].as_str().unwrap().to_string();
    let said = f.consent.decide(&id, true, None, "mac", None).unwrap();
    assert_eq!(said["answered"], json!(true));
    f.consent.wait_for(&id, Duration::from_secs(5)).unwrap()
}

#[test]
fn a_failed_dispatch_keeps_the_real_reason_code_on_the_request() {
    let f = failing(
        "reason",
        ApiError::new(ErrorCode::Failed, SIGNED_OUT).with_reason("worker_claude_signed_out"),
    );
    let done = dispatch_failed(&f, "k-signed-out");
    assert_eq!(done.state.as_str(), "failed");
    let view = f.consent.project(&done, true);
    assert_eq!(view["error"]["code"], "worker_claude_signed_out", "{view}");
    let said = view["error"]["message"].as_str().unwrap();
    assert!(said.contains("Sign in"), "{view}");
    let shown = f.consent.project(&done, false);
    assert_eq!(shown["error"], view["error"], "{shown}");
}

#[test]
fn a_spawn_failure_without_a_named_reason_keeps_the_failed_code_and_a_plain_line() {
    let f = failing(
        "spawn",
        ApiError::new(ErrorCode::Failed, "claude --bg exited with code 1."),
    );
    let done = dispatch_failed(&f, "k-spawn");
    let view = f.consent.project(&done, true);
    assert_eq!(view["error"]["code"], "failed");
    let said = view["error"]["message"].as_str().unwrap();
    assert!(!said.is_empty() && !said.contains("exited"), "{view}");
}

#[test]
fn the_consent_command_reports_the_reason_not_the_generic_line() {
    let f = failing(
        "cli",
        ApiError::new(ErrorCode::Failed, SIGNED_OUT).with_reason("worker_claude_signed_out"),
    );
    let done = dispatch_failed(&f, "k-cli");
    let view = f.consent.project(&done, true);
    let failure = crate::consent_cli::failure_of(&view, ops::CONSENT_GET);
    assert_eq!(failure.message, view["error"]["message"].as_str().unwrap());
    assert_ne!(failure.message, "The request didn't go through.");
    assert_eq!(
        failure.request.as_ref().unwrap()["error"]["code"],
        "worker_claude_signed_out"
    );
}

#[test]
fn a_claude_dispatch_takes_the_task_text_and_hands_it_on() {
    let r = rig("claude-text");
    let mut args = args_for("claude", "k-claude-text");
    args["text"] = json!("Review the inbox design.");
    let reply = r.call(ops::CLUSTER_DISPATCH, args).unwrap();
    let id = r.card_of(&reply);
    let done = r.allow(&id, "mac");
    assert_eq!(done.state.as_str(), "applied");
    let sent = r.hands.calls.lock().unwrap().last().unwrap().1.clone();
    assert_eq!(sent["tool"], "claude");
    assert_eq!(sent["text"], "Review the inbox design.");
}
