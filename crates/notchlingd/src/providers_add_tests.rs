use crate::acting_ops_tests::FakeRoom;
use crate::providers_ops::*;
use crate::providers_ops_tests::{codex_login, no_secrets, Rig};
use crate::switches::ACCOUNTS;
use notchling_engine::accounts::procs::{FakeApps, FakeProcs, ProcCall};
use notchling_engine::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, Confirmation,
    ProviderAccount, ProviderId, ProviderRegistry, ProviderState,
};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn adding_a_codex_account_reports_the_sign_in_page_then_done() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("add", true, procs, FakeApps::new());
    let reply = rig
        .call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    assert_eq!(reply["state"], "waiting");
    let login = rig.procs.login_state();
    login.lock().unwrap().output = "Open https://auth.openai.com/oauth/authorize?z=1\n".into();
    let waiting = rig.wait_for(|e| e["reason"] == "add" && e["state"] == "waiting");
    assert_eq!(
        waiting["sign_in_url"],
        "https://auth.openai.com/oauth/authorize?z=1"
    );
    let scratch = rig
        .procs
        .calls()
        .into_iter()
        .find_map(|c| match c {
            ProcCall::SpawnLogin(s) => Some(PathBuf::from(&s.env[0].1)),
            _ => None,
        })
        .unwrap();
    std::fs::write(
        scratch.join("auth.json"),
        codex_login("c@example.com", "u3", "w3"),
    )
    .unwrap();
    login.lock().unwrap().exit = Some(0);
    let done = rig.wait_for(|e| e["reason"] == "add" && e["state"] == "done");
    no_secrets(&done);
    assert!(rig.keychain.get("Notchling Codex", "u3:w3").is_some());
    assert!(
        !rig.codex_auth().exists(),
        "the live login was never written"
    );
}

#[test]
fn an_add_flow_stops_touching_logins_the_moment_accounts_turn_off() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("addoff", true, procs, FakeApps::new());
    rig.call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    rig.flag.set(false);
    std::thread::sleep(Duration::from_millis(60));
    let calls = rig.keychain.calls().len();
    let scratch = rig
        .procs
        .calls()
        .into_iter()
        .find_map(|c| match c {
            ProcCall::SpawnLogin(s) => Some(PathBuf::from(&s.env[0].1)),
            _ => None,
        })
        .unwrap();
    assert!(!scratch.exists());
    rig.procs.login_state().lock().unwrap().exit = Some(0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        rig.keychain.calls().len(),
        calls,
        "no Keychain call after the switch went off"
    );
    assert!(!rig.events().iter().any(|e| e["state"] == "done"));
    assert_eq!(
        rig.call(ops::PROVIDERS_ADD_CANCEL, json!({"provider": "codex"}))
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}

#[test]
fn a_waiting_add_flow_ends_when_accounts_turn_off_not_at_its_next_poll() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("addoffnow", true, procs, FakeApps::new());
    rig.ops
        .set_add_timing(Duration::from_secs(600), Duration::from_secs(900));
    rig.call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    let started = Instant::now();
    rig.flag.set(false);
    let ended = rig.wait_for(|e| e["reason"] == "add" && e["state"] == "cancelled");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(ended["error"], ACCOUNTS_OFF);
    no_secrets(&ended);
    assert!(rig.procs.login_state().lock().unwrap().killed);
    let scratch: Vec<_> = std::fs::read_dir(rig.root.join("state/codex-login"))
        .map(|d| d.flatten().collect())
        .unwrap_or_default();
    assert!(scratch.is_empty(), "{scratch:?}");
    assert!(!rig
        .keychain
        .calls()
        .iter()
        .any(|c| c.starts_with("write") || c.starts_with("delete")));
}

#[test]
fn stopping_the_daemon_cancels_a_waiting_add_only_while_accounts_are_on() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("close", true, procs, FakeApps::new());
    rig.call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    rig.ops.close();
    assert!(rig.procs.login_state().lock().unwrap().killed);

    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let off = Rig::new("closeoff", true, procs, FakeApps::new());
    off.call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    off.flag.set(false);
    off.ops.close();
    assert!(!off
        .keychain
        .calls()
        .iter()
        .any(|c| c.starts_with("write") || c.starts_with("delete")));
}

pub(crate) enum Step {
    Page(&'static str),
    Store(&'static str),
    Cancel,
}

pub(crate) struct Paced {
    name: &'static str,
    accounts: Mutex<Vec<String>>,
    active: Mutex<Option<String>>,
    steps: Mutex<VecDeque<Step>>,
    pub(crate) refuses: Mutex<bool>,
    saves_first: Mutex<Option<&'static str>>,
    pub(crate) on_sync: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    pub(crate) began: Mutex<u32>,
    names: Mutex<Option<Option<String>>>,
    pub(crate) on_begin: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    pub(crate) on_cancel: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    pub(crate) on_state: Mutex<Option<Box<dyn FnOnce() -> bool + Send>>>,
    synced: Mutex<u32>,
    pub(crate) confirms: Mutex<bool>,
    pub(crate) cancels: Mutex<u32>,
    pub(crate) abandons: Mutex<u32>,
}

impl Paced {
    fn named(name: &'static str) -> Arc<Paced> {
        Arc::new(Paced {
            name,
            accounts: Mutex::new(Vec::new()),
            active: Mutex::new(None),
            steps: Mutex::new(VecDeque::new()),
            refuses: Mutex::new(false),
            saves_first: Mutex::new(None),
            on_sync: Mutex::new(None),
            began: Mutex::new(0),
            names: Mutex::new(None),
            on_begin: Mutex::new(None),
            on_cancel: Mutex::new(None),
            on_state: Mutex::new(None),
            synced: Mutex::new(0),
            confirms: Mutex::new(false),
            cancels: Mutex::new(0),
            abandons: Mutex::new(0),
        })
    }

    fn store(&self, id: &str) {
        self.accounts.lock().unwrap().push(id.to_string());
    }

    fn then(&self, step: Step) {
        self.script([step]);
    }

    fn script<const N: usize>(&self, steps: [Step; N]) {
        self.steps.lock().unwrap().extend(steps);
    }
}

impl AccountProvider for Paced {
    fn id(&self) -> ProviderId {
        self.name.into()
    }
    fn title(&self) -> &'static str {
        "Paced"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT
    }
    fn state(&self) -> ProviderState {
        let hook = self.on_state.lock().unwrap().take();
        ProviderState {
            adding: hook.is_some_and(|hook| hook()),
            accounts: self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .map(|id| ProviderAccount::new(id, "pat@example.com", None))
                .collect(),
            active_id: self.active.lock().unwrap().clone(),
            ..ProviderState::default()
        }
    }
    fn sync(&self, _force: bool) {
        *self.synced.lock().unwrap() += 1;
        let hook = self.on_sync.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }
    fn switch_to(&self, _id: &str) -> Result<(), AccountError> {
        Ok(())
    }
    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        *self.began.lock().unwrap() += 1;
        let hook = self.on_begin.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        if *self.refuses.lock().unwrap() {
            return Err(AccountError::new("Couldn't start."));
        }
        if let Some(id) = self.saves_first.lock().unwrap().take() {
            self.store(id);
        }
        Ok(AddStarted::default())
    }
    fn poll_add(&self) -> AddProgress {
        match self.steps.lock().unwrap().pop_front() {
            Some(Step::Page(url)) => AddProgress::Waiting {
                sign_in_url: Some(url.to_string()),
            },
            Some(Step::Store(id)) => {
                self.store(id);
                AddProgress::Done
            }
            Some(Step::Cancel) => AddProgress::Cancelled,
            None => AddProgress::Waiting { sign_in_url: None },
        }
    }
    fn cancel_add(&self) {
        *self.cancels.lock().unwrap() += 1;
        let hook = self.on_cancel.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }
    fn abandon_add(&self) -> Option<String> {
        *self.abandons.lock().unwrap() += 1;
        Some("Left signed out.".into())
    }
    fn add_confirmation(&self) -> Option<Confirmation> {
        self.confirms.lock().unwrap().then(|| Confirmation {
            title: "Sign out?".into(),
            message: "It quits.".into(),
            button: "Continue".into(),
        })
    }
    fn added_account(&self) -> Option<Option<String>> {
        self.names.lock().unwrap().clone()
    }
    fn remove(&self, _id: &str) {}
    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch Paced"
    }
}

fn paced_rig() -> (ProvidersOps, Arc<FakeRoom>, Arc<Paced>, Arc<Paced>) {
    let (one, two) = (Paced::named("one"), Paced::named("two"));
    let mut registry = ProviderRegistry::new();
    registry.register(one.clone());
    registry.register(two.clone());
    let room = Arc::new(FakeRoom::default());
    let ops = ProvidersOps::new(
        SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, true),
        registry,
        room.clone(),
    );
    ops.set_add_timing(Duration::from_millis(5), Duration::from_secs(30));
    (ops, room, one, two)
}

pub(crate) fn begin(ops: &ProvidersOps, provider: &str) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match ops.call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": provider})) {
            Ok(reply) => return Ok(reply),
            Err(e) if !e.message.contains("another account change") => return Err(e.message),
            Err(e) => assert!(Instant::now() < deadline, "{}", e.message),
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn attempt_of(reply: &Value) -> u64 {
    reply["attempt"].as_u64().unwrap()
}

pub(crate) fn adds_of(room: &FakeRoom, attempt: u64) -> Vec<Value> {
    room.named("providers.changed")
        .into_iter()
        .filter(|e| e["reason"] == "add" && e["attempt"] == attempt)
        .collect()
}

pub(crate) fn ends(room: &FakeRoom, attempt: u64, state: &str) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let states: Vec<String> = adds_of(room, attempt)
            .iter()
            .map(|e| e["state"].as_str().unwrap().to_string())
            .collect();
        if states.last().map(String::as_str) == Some(state) {
            return states;
        }
        assert!(Instant::now() < deadline, "{states:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_add_names_its_attempt_in_the_reply_and_in_every_event() {
    let (ops, room, one, two) = paced_rig();
    let reply = begin(&ops, "one").unwrap();
    assert_eq!(
        reply,
        json!({"state": "waiting", "provider": "one", "attempt": 1, "sign_in_url": null})
    );
    let other = begin(&ops, "two").unwrap();
    assert_eq!(attempt_of(&other), 2, "one count for every provider");
    one.script([Step::Page("https://sign.in/one"), Step::Store("p-new")]);
    assert_eq!(ends(&room, 1, "done"), ["started", "waiting", "done"]);
    two.then(Step::Cancel);
    assert_eq!(ends(&room, 2, "cancelled"), ["started", "cancelled"]);
    let all = room.named("providers.changed");
    assert!(all
        .iter()
        .all(|e| e["reason"] != "add" || e["attempt"].is_u64()));
    let said = format!("{all:?}{reply}{other}");
    for bad in ["p-new", "pat@"] {
        assert!(!said.contains(bad), "{bad} in {said}");
    }

    let again = begin(&ops, "one").unwrap();
    assert_eq!(attempt_of(&again), 3, "a later add has a later attempt");
    one.then(Step::Cancel);
    ends(&room, 3, "cancelled");
    *one.refuses.lock().unwrap() = true;
    assert_eq!(begin(&ops, "one").unwrap_err(), "Couldn't start.");
    assert_eq!(ends(&room, 4, "failed"), ["failed"]);
}

#[test]
fn an_add_keeps_which_accounts_it_stored_and_no_others() {
    let (ops, room, one, _) = paced_rig();
    one.store("p-old");
    *one.active.lock().unwrap() = Some("p-live".to_string());
    let first = attempt_of(&begin(&ops, "one").unwrap());
    assert_eq!(ops.added(first), None, "nothing until it is done");
    one.then(Step::Store("p-live"));
    ends(&room, first, "done");
    assert_eq!(
        ops.added(first),
        Some(Vec::new()),
        "the login in use when it began is not new"
    );

    one.store("p-other");
    let second = attempt_of(&begin(&ops, "one").unwrap());
    one.then(Step::Store("p-new"));
    ends(&room, second, "done");
    assert_eq!(ops.added(second), Some(vec!["p-new".to_string()]));
    assert_eq!(ops.added(first), Some(Vec::new()));

    let third = attempt_of(&begin(&ops, "one").unwrap());
    one.then(Step::Cancel);
    ends(&room, third, "cancelled");
    assert_eq!(ops.added(third), None);
    assert_eq!(ops.added(third + 1), None);

    *one.saves_first.lock().unwrap() = Some("p-saved-at-start");
    let fourth = attempt_of(&begin(&ops, "one").unwrap());
    one.then(Step::Store("p-newer"));
    ends(&room, fourth, "done");
    assert_eq!(
        ops.added(fourth),
        Some(vec!["p-newer".to_string()]),
        "a login the tool saved while starting is not new"
    );
}

#[test]
fn the_record_of_what_was_stored_is_made_before_the_done_event() {
    struct Asking {
        ops: Mutex<Option<Arc<ProvidersOps>>>,
        seen: Mutex<Vec<Option<Vec<String>>>>,
        inner: FakeRoom,
    }
    impl crate::acting_ops::Room for Asking {
        fn publish(&self, topic: &str, event: &str, data: &Value) {
            if data["state"] == "done" {
                let ops = self.ops.lock().unwrap().clone().unwrap();
                let stored = ops.added(data["attempt"].as_u64().unwrap());
                self.seen.lock().unwrap().push(stored);
            }
            self.inner.publish(topic, event, data);
        }
        fn can_answer(&self) -> bool {
            false
        }
        fn can_follow(&self, _topic: &str) -> bool {
            false
        }
    }
    let one = Paced::named("one");
    let mut registry = ProviderRegistry::new();
    registry.register(one.clone());
    let room = Arc::new(Asking {
        ops: Mutex::new(None),
        seen: Mutex::new(Vec::new()),
        inner: FakeRoom::default(),
    });
    let ops = Arc::new(ProvidersOps::new(
        SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, true),
        registry,
        room.clone(),
    ));
    ops.set_add_timing(Duration::from_millis(5), Duration::from_secs(30));
    *room.ops.lock().unwrap() = Some(ops.clone());
    let attempt = attempt_of(&begin(&ops, "one").unwrap());
    one.then(Step::Store("p-new"));
    ends(&room.inner, attempt, "done");
    assert_eq!(
        *room.seen.lock().unwrap(),
        vec![Some(vec!["p-new".to_string()])]
    );
    *room.ops.lock().unwrap() = None;
}

#[test]
fn only_the_last_adds_that_ended_done_keep_their_record() {
    let (ops, room, one, _) = paced_rig();
    let mut attempts = Vec::new();
    for _ in 0..17 {
        let attempt = attempt_of(&begin(&ops, "one").unwrap());
        one.then(Step::Store("p-again"));
        ends(&room, attempt, "done");
        attempts.push(attempt);
    }
    assert_eq!(ops.added(attempts[0]), None);
    assert!(ops.added(attempts[1]).is_some());
    assert!(ops.added(attempts[16]).is_some());
}

#[test]
fn accounts_turned_off_during_the_read_before_an_add_stop_it_before_it_begins() {
    let one = Paced::named("one");
    let mut registry = ProviderRegistry::new();
    registry.register(one.clone());
    let room = Arc::new(FakeRoom::default());
    let flag = SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, true);
    let ops = ProvidersOps::new(flag.clone(), registry, room.clone());
    ops.set_add_timing(Duration::from_millis(5), Duration::from_secs(30));
    *one.on_sync.lock().unwrap() = Some(Box::new(move || {
        flag.set(false);
    }));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(refused.message, ACCOUNTS_OFF);
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    assert_eq!(adds_of(&room, 1)[0]["error"], ACCOUNTS_OFF);
    assert_eq!(*one.began.lock().unwrap(), 0, "the add never began");
}

#[test]
fn an_add_whose_provider_names_no_account_stored_none_whatever_its_list_shows() {
    let (ops, room, one, _) = paced_rig();
    *one.names.lock().unwrap() = Some(None);
    let attempt = attempt_of(&begin(&ops, "one").unwrap());
    one.then(Step::Store("p-terminal"));
    ends(&room, attempt, "done");
    assert_eq!(ops.added(attempt), Some(Vec::new()));

    *one.names.lock().unwrap() = Some(Some("p-named".to_string()));
    let attempt = attempt_of(&begin(&ops, "one").unwrap());
    one.then(Step::Store("p-other"));
    ends(&room, attempt, "done");
    assert_eq!(ops.added(attempt), Some(vec!["p-named".to_string()]));
}

pub(crate) fn lone_rig() -> (
    Arc<ProvidersOps>,
    Arc<FakeRoom>,
    Arc<Paced>,
    Arc<SwitchFlag>,
) {
    let one = Paced::named("one");
    let mut registry = ProviderRegistry::new();
    registry.register(one.clone());
    let room = Arc::new(FakeRoom::default());
    let flag = SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, true);
    let ops = Arc::new(ProvidersOps::new(flag.clone(), registry, room.clone()));
    ops.set_add_timing(Duration::from_millis(5), Duration::from_secs(30));
    (ops, room, one, flag)
}

#[test]
fn an_add_asked_after_stopping_is_refused_before_it_reads_the_provider() {
    let (ops, room, one, _) = lone_rig();
    ops.close();

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.message, "Notchling is stopping.");
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    assert_eq!(*one.synced.lock().unwrap(), 0);
    assert_eq!(*one.began.lock().unwrap(), 0);
}

#[test]
fn stopping_while_an_add_begins_refuses_it_and_ends_it_cancelled() {
    let (ops, room, one, _) = lone_rig();
    let closer = ops.clone();
    *one.on_begin.lock().unwrap() = Some(Box::new(move || closer.close()));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.message, "Notchling is stopping.");
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
}

#[test]
fn accounts_turned_off_while_an_add_begins_refuse_it_as_off_not_failed() {
    let (ops, room, one, flag) = lone_rig();
    *one.refuses.lock().unwrap() = true;
    *one.on_begin.lock().unwrap() = Some(Box::new(move || {
        flag.set(false);
    }));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(refused.message, ACCOUNTS_OFF);
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    assert_eq!(adds_of(&room, 1)[0]["error"], ACCOUNTS_OFF);
}
