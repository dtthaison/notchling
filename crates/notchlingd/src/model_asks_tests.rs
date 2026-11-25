use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{agent_caller, app_caller, verified};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use notchling_engine::consent::ConsentStore;
use notchling_engine::search::models::manifest::manifest;
use notchling_events::{MemorySink, Salt};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

struct Rig {
    consent: Arc<ConsentOps>,
    asks: Arc<ModelAsks>,
    status: Arc<Mutex<Value>>,
    starts: Arc<AtomicUsize>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn model() -> String {
    manifest().models[0].id.clone()
}

fn rig(tag: &str) -> (Rig, crate::limits_ops_tests::TempDir) {
    let tmp = crate::limits_ops_tests::TempDir::new(&format!("model-asks-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        Some(inbox),
        Some(Events::fixed(Arc::new(MemorySink::new()), Salt([3; 32]))),
        Arc::new(crate::acting_ops::now_ms),
    );
    consent.bind_identity(Arc::new(|_| verified("parent")));
    consent.start();
    let status = Arc::new(Mutex::new(json!({
        "state": "no_model",
        "models": [{"id": model(), "present": false}],
        "fetch": [],
    })));
    let read = status.clone();
    let asks = ModelAsks::new(
        consent.clone(),
        Arc::new(move || read.lock().unwrap().clone()),
    );
    (
        Rig {
            consent,
            asks,
            status,
            starts: Arc::new(AtomicUsize::new(0)),
        },
        tmp,
    )
}

impl Rig {
    fn ask(&self, args: Value) -> Result<Value, ApiError> {
        self.asks.call(&agent_caller(), ops::EMBED_REQUEST, &args)
    }

    fn card(&self) -> String {
        let reply = self.ask(json!({"model": model()})).unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        reply["request"]["id"].as_str().unwrap().to_string()
    }

    fn apply(&self, id: &str) -> Result<Value, ApiError> {
        let row = self.consent.get_row(id).unwrap();
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(id);
        let starts = self.starts.clone();
        Cited::apply(&*self.asks, &app_caller(), &op, &args, &move |_| {
            starts.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"state": "downloading"}))
        })
    }
}

fn reason(e: &ApiError) -> String {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn the_request_is_an_agent_act_and_the_fetch_is_claimed_only_with_a_card() {
    let (r, _tmp) = rig("tier");
    assert_eq!(r.asks.spec(ops::EMBED_REQUEST), Some(OpSpec::AGENT_ACTS));
    assert_eq!(r.asks.spec(ops::EMBED_FETCH), None);
    let cited: &dyn Cited = &*r.asks;
    assert!(cited.claims(ops::EMBED_FETCH, &json!({"model": "m", "consentId": "c1"})));
    assert!(!cited.claims(ops::EMBED_FETCH, &json!({"model": "m"})));
    assert!(!cited.claims(ops::EMBED_STATUS, &json!({"consentId": "c1"})));
}

#[test]
fn asking_opens_a_card_that_names_the_model_and_starts_nothing() {
    let (r, _tmp) = rig("ask");
    let id = r.card();
    let row = r.consent.get_row(&id).unwrap();
    assert_eq!(row.kind, MODEL);
    assert_eq!(row.change["apply"]["op"], ops::EMBED_FETCH);
    assert_eq!(row.change["apply"]["args"], json!({"model": model()}));
    assert_eq!(row.facts[0]["line"], "consent-model-download");
    assert_eq!(r.starts.load(Ordering::SeqCst), 0);
}

#[test]
fn a_yes_starts_the_download_once() {
    let (r, _tmp) = rig("yes");
    let id = r.card();
    let reply = r.apply(&id).unwrap();
    assert_eq!(reply["state"], "downloading");
    assert_eq!(r.starts.load(Ordering::SeqCst), 1);
    assert!(r.apply(&id).is_err());
    assert_eq!(r.starts.load(Ordering::SeqCst), 1);
}

#[test]
fn a_no_never_starts_the_download() {
    let (r, _tmp) = rig("no");
    let id = r.card();
    assert!(r.consent.deny_by_app(&id));
    assert!(r.apply(&id).is_err());
    assert_eq!(r.starts.load(Ordering::SeqCst), 0);
}

#[test]
fn a_card_for_one_model_cannot_fetch_another() {
    let (r, _tmp) = rig("other");
    let id = r.card();
    let err = Cited::apply(
        &*r.asks,
        &app_caller(),
        ops::EMBED_FETCH,
        &json!({"model": "static-mrl@b68f412/512i8/p1", "consentId": id}),
        &|_| Ok(json!({})),
    )
    .unwrap_err();
    assert_eq!(reason(&err), "needs_consent");
}

#[test]
fn an_agent_cannot_apply_its_own_card() {
    let (r, _tmp) = rig("agent");
    let id = r.card();
    let err = Cited::apply(
        &*r.asks,
        &agent_caller(),
        ops::EMBED_FETCH,
        &json!({"model": model(), "consentId": id}),
        &|_| Ok(json!({})),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
}

#[test]
fn nothing_is_asked_while_search_is_off_or_the_model_is_here_or_coming() {
    let (r, _tmp) = rig("refused");
    r.status.lock().unwrap()["state"] = json!("off");
    assert_eq!(
        reason(&r.ask(json!({"model": model()})).unwrap_err()),
        "semantic_off"
    );
    r.status.lock().unwrap()["state"] = json!("no_model");
    r.status.lock().unwrap()["models"][0]["present"] = json!(true);
    assert_eq!(
        reason(&r.ask(json!({"model": model()})).unwrap_err()),
        "present"
    );
    r.status.lock().unwrap()["models"][0]["present"] = json!(false);
    r.status.lock().unwrap()["fetch"] = json!([{"model": model(), "state": "downloading"}]);
    assert_eq!(
        reason(&r.ask(json!({"model": model()})).unwrap_err()),
        "running"
    );
    assert!(r.ask(json!({"model": "nope"})).is_err());
    assert!(r.ask(json!({})).is_err());
}

#[test]
fn a_dry_run_shows_the_card_and_opens_none() {
    let (r, _tmp) = rig("dry");
    let reply = r.ask(json!({"model": model(), "dryRun": true})).unwrap();
    assert_eq!(reply["dryRun"], true);
    assert_eq!(r.consent.store().open_rows().unwrap().len(), 0);
}

#[test]
fn a_key_gives_back_the_same_card() {
    let (r, _tmp) = rig("key");
    let args = json!({"model": model(), "consent": {"key": "k1"}});
    let first = r.ask(args.clone()).unwrap();
    let again = r.ask(args).unwrap();
    assert_eq!(first["request"]["id"], again["request"]["id"]);
    assert_eq!(r.consent.store().open_rows().unwrap().len(), 1);
}

#[test]
fn a_yes_after_the_model_arrived_is_stale() {
    let (r, _tmp) = rig("stale");
    let id = r.card();
    r.status.lock().unwrap()["models"][0]["present"] = json!(true);
    let err = r.apply(&id).unwrap_err();
    assert_eq!(reason(&err), "stale");
    assert_eq!(r.starts.load(Ordering::SeqCst), 0);
}
