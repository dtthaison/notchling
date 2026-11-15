use super::tests::{agent, err, seed, setup};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Stub {
    answer: Option<Result<Value, ApiError>>,
    asked: AtomicUsize,
}

impl WorkerUse for Stub {
    fn use_secrets(&self, _caller: &Caller, _args: &Value) -> Option<Result<Value, ApiError>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.answer.clone()
    }
}

fn stub(answer: Option<Result<Value, ApiError>>) -> Arc<Stub> {
    Arc::new(Stub {
        answer,
        asked: AtomicUsize::new(0),
    })
}

fn wanted() -> Value {
    json!({"names": ["gh"], "folder": "/w", "command": "make"})
}

#[test]
fn a_worker_answers_vault_use_from_its_grants_with_the_vault_switch_off() {
    let s = setup("worker-on", false);
    let worker = stub(Some(Ok(json!({"env": {"T": "x"}, "hide": {}}))));
    s.ops.set_worker(worker.clone());
    let got = s.ops.call(&agent(), ops::VAULT_USE, &wanted()).unwrap();
    assert_eq!(got["env"]["T"], "x");
    assert_eq!(worker.asked.load(Ordering::SeqCst), 1);
    assert_eq!(*s.item.reads.lock().unwrap(), 0, "the keychain wasn't read");
}

#[test]
fn a_worker_refusal_is_final_and_never_falls_back_to_the_local_vault() {
    let s = setup("worker-refused", true);
    seed(&s, false);
    let reads = *s.item.reads.lock().unwrap();
    let refusal = ApiError::new(ErrorCode::Forbidden, "not part of a cluster job");
    s.ops.set_worker(stub(Some(Err(refusal))));
    let e = err(s.ops.call(&agent(), ops::VAULT_USE, &wanted()));
    assert_eq!(e.message, "not part of a cluster job");
    assert_eq!(*s.item.reads.lock().unwrap(), reads, "no local read");
}

#[test]
fn the_vault_node_and_a_machine_in_no_cluster_stay_as_today() {
    let s = setup("worker-none", true);
    seed(&s, false);
    let worker = stub(None);
    s.ops.set_worker(worker.clone());
    let got = s.ops.call(&agent(), ops::VAULT_USE, &wanted()).unwrap();
    assert!(got["env"].is_object(), "the local vault answered");
    assert_eq!(worker.asked.load(Ordering::SeqCst), 1);
    s.switch.set(false);
    let e = err(s.ops.call(&agent(), ops::VAULT_USE, &wanted()));
    assert_eq!(e.code, ErrorCode::Forbidden);
}

#[test]
fn only_vault_use_is_asked_of_the_worker() {
    let s = setup("worker-only-use", false);
    let worker = stub(Some(Ok(json!({}))));
    s.ops.set_worker(worker.clone());
    for op in [ops::VAULT_FIND, ops::VAULT_REQUEST, ops::VAULT_LIST] {
        assert!(s.ops.call(&agent(), op, &json!({})).is_err(), "{op}");
    }
    assert_eq!(worker.asked.load(Ordering::SeqCst), 0);
}
