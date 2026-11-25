use super::store::Accounts;
use crate::actions::Transport;
use crate::switches::Switches;
use notchling_local_api::{ApiError, ClientError, ErrorCode};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

type Calls = Arc<Mutex<Vec<(String, Value)>>>;
type Script = Box<dyn Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync>;

struct Fake {
    calls: Calls,
    answer: Script,
}

impl Transport for Fake {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let reply = (self.answer)(op, &args);
        self.calls.lock().unwrap().push((op.to_owned(), args));
        reply
    }
}

fn rig_try(
    answer: impl Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync + 'static,
) -> (Accounts, Calls) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let store = Accounts::over(Box::new(Fake {
        calls: calls.clone(),
        answer: Box::new(answer),
    }));
    (store, calls)
}

fn rig(answer: impl Fn(&str, &Value) -> Value + Send + Sync + 'static) -> (Accounts, Calls) {
    rig_try(move |op, args| Ok(answer(op, args)))
}

fn begins(replies: Vec<Value>) -> impl Fn(&str, &Value) -> Value + Send + Sync {
    let next = Mutex::new(replies.into_iter());
    move |op, _| match op {
        "providers.add.begin" => next.lock().unwrap().next().unwrap(),
        _ => json!({"cancelled": true}),
    }
}

fn last_cancel(calls: &Calls) -> Value {
    calls
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|c| c.0 == "providers.add.cancel")
        .map(|c| c.1.clone())
        .unwrap()
}

fn waiting(attempt: u64) -> Value {
    json!({"state": "waiting", "attempt": attempt, "sign_in_url": "https://x"})
}

#[test]
fn a_cancel_names_the_attempt_its_begin_returned() {
    let (store, calls) = rig(begins(vec![waiting(7)]));
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(
        last_cancel(&calls),
        json!({"provider": "codex", "attempt": 7})
    );
}

#[test]
fn a_cancel_names_the_newest_attempt_of_its_own_provider() {
    let (store, calls) = rig(begins(vec![waiting(7), waiting(8), waiting(9)]));
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    store.add_provider(&on, "agy", false).unwrap();
    store.add_provider(&on, "codex", false).unwrap();
    store.cancel_add_provider(&on, "agy").unwrap();
    assert_eq!(last_cancel(&calls)["attempt"], 8);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls)["attempt"], 9);
}

#[test]
fn a_cancel_for_an_add_this_app_never_began_names_no_attempt() {
    let (store, calls) = rig(begins(vec![]));
    store
        .cancel_add_provider(&Switches::all_on(), "codex")
        .unwrap();
    assert_eq!(last_cancel(&calls), json!({"provider": "codex"}));
}

#[test]
fn a_begin_that_answers_no_attempt_forgets_the_old_one() {
    let ask = json!({"state": "needs_confirmation", "confirmation": {"title": "t", "message": "m", "button": "b"}});
    let (store, calls) = rig(begins(vec![waiting(7), ask]));
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    store.add_provider(&on, "codex", false).unwrap();
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls), json!({"provider": "codex"}));
}

#[test]
fn a_cancel_still_ending_keeps_its_attempt_and_one_that_ended_drops_it() {
    let ending = Mutex::new(true);
    let (store, calls) = rig(move |op, _| match op {
        "providers.add.begin" => waiting(7),
        _ if std::mem::take(&mut *ending.lock().unwrap()) => json!({"state": "cancelling"}),
        _ => json!({"cancelled": true}),
    });
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    store.cancel_add_provider(&on, "codex").unwrap();
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls)["attempt"], 7);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls), json!({"provider": "codex"}));
}

fn listing(adding: bool, running: Option<u64>) -> Value {
    json!({"providers": [{"id": "codex", "state": {"adding": adding, "add_attempt": running}}]})
}

fn refused() -> ClientError {
    ClientError::Api(ApiError::new(ErrorCode::Failed, "refused"))
}

#[test]
fn a_cancel_after_the_add_ended_names_no_attempt() {
    let adding = Arc::new(AtomicBool::new(true));
    let seen = adding.clone();
    let (store, calls) = rig(move |op, _| match op {
        "providers.add.begin" => waiting(7),
        "providers.list" => {
            let on = seen.load(Ordering::SeqCst);
            listing(on, on.then_some(7))
        }
        "providers.add.cancel" => json!({"cancelled": true}),
        _ => json!({}),
    });
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    store.refresh(&on);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls)["attempt"], 7, "still adding");
    store.add_provider(&on, "codex", false).unwrap();
    adding.store(false, Ordering::SeqCst);
    store.refresh(&on);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls), json!({"provider": "codex"}));
}

#[test]
fn a_list_read_that_began_before_the_begin_returned_keeps_its_attempt() {
    let (started_tx, started_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let started_tx = Mutex::new(started_tx);
    let go_rx = Mutex::new(go_rx);
    let first = AtomicBool::new(true);
    let (store, calls) = rig(move |op, _| match op {
        "providers.add.begin" => waiting(7),
        "providers.list" => {
            if first.swap(false, Ordering::SeqCst) {
                started_tx.lock().unwrap().send(()).unwrap();
                go_rx.lock().unwrap().recv().unwrap();
            }
            listing(false, None)
        }
        "providers.add.cancel" => json!({"cancelled": true}),
        _ => json!({}),
    });
    let store = Arc::new(store);
    let on = Switches::all_on();
    let reader = {
        let store = store.clone();
        std::thread::spawn(move || store.refresh(&on))
    };
    started_rx.recv().unwrap();
    store.add_provider(&on, "codex", false).unwrap();
    go_tx.send(()).unwrap();
    reader.join().unwrap();
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls)["attempt"], 7);
}

#[test]
fn a_begin_that_fails_keeps_the_attempt_of_the_add_still_running() {
    let begun = Mutex::new(0);
    let (store, calls) = rig_try(move |op, _| match op {
        "providers.add.begin" => match std::mem::replace(&mut *begun.lock().unwrap(), 1) {
            0 => Ok(waiting(7)),
            _ => Err(refused()),
        },
        _ => Ok(json!({"cancelled": true})),
    });
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    assert!(store.add_provider(&on, "codex", false).is_err());
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(
        last_cancel(&calls),
        json!({"provider": "codex", "attempt": 7})
    );
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls), json!({"provider": "codex"}), "ended");
}

#[test]
fn a_begin_refused_during_a_slow_begin_keeps_its_attempt_through_a_list_read() {
    let begun = Mutex::new(0);
    let (store, calls) = rig_try(move |op, _| match op {
        "providers.add.begin" => match std::mem::replace(&mut *begun.lock().unwrap(), 1) {
            0 => Ok(json!({"state": "starting", "attempt": 7})),
            _ => Err(refused()),
        },
        "providers.list" => Ok(listing(false, Some(7))),
        _ => Ok(json!({"state": "cancelling"})),
    });
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    store.refresh(&on);
    assert!(store.add_provider(&on, "codex", false).is_err());
    store.refresh(&on);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls)["attempt"], 7);
}

#[test]
fn losing_the_daemon_forgets_every_attempt() {
    let down = Arc::new(AtomicBool::new(false));
    let seen = down.clone();
    let (store, calls) = rig_try(move |op, _| match op {
        "providers.add.begin" => Ok(waiting(7)),
        "providers.add.cancel" => Ok(json!({"cancelled": true})),
        _ if seen.load(Ordering::SeqCst) => Err(ClientError::Closed),
        _ => Ok(json!({})),
    });
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    down.store(true, Ordering::SeqCst);
    store.refresh(&on);
    down.store(false, Ordering::SeqCst);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls), json!({"provider": "codex"}));
}

#[test]
fn a_slow_begin_keeps_its_attempt_while_the_daemon_still_runs_it() {
    let (store, calls) = rig(move |op, _| match op {
        "providers.add.begin" => json!({"state": "starting", "attempt": 7}),
        "providers.list" => listing(false, Some(7)),
        _ => json!({"cancelled": true}),
    });
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    store.refresh(&on);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls)["attempt"], 7);
}

#[test]
fn a_slow_cancel_keeps_its_attempt_until_the_daemon_has_ended_the_add() {
    let ended = Arc::new(AtomicBool::new(false));
    let seen = ended.clone();
    let (store, calls) = rig(move |op, _| {
        let over = seen.load(Ordering::SeqCst);
        match op {
            "providers.add.begin" => waiting(7),
            "providers.list" => listing(false, (!over).then_some(7)),
            "providers.add.cancel" if !over => json!({"state": "cancelling"}),
            _ => json!({"cancelled": true}),
        }
    });
    let on = Switches::all_on();
    store.add_provider(&on, "codex", false).unwrap();
    store.cancel_add_provider(&on, "codex").unwrap();
    store.refresh(&on);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls)["attempt"], 7, "still cancelling");
    ended.store(true, Ordering::SeqCst);
    store.refresh(&on);
    store.cancel_add_provider(&on, "codex").unwrap();
    assert_eq!(last_cancel(&calls), json!({"provider": "codex"}));
}
