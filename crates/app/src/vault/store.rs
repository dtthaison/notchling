use std::sync::{Arc, Mutex};

use notchling_engine::vault::entry::{VaultEntry, VaultValue};
use notchling_local_api::{ApiError, ClientError, SOCKET_NAME};
use notchling_locale as t;
use serde_json::Value;

use super::consent_save::{failure, wants_state, Failure, Refusal};
use super::wire::{parse_entries, Call, Decision};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;
use crate::switches::{self, Switch, Switches};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub attached: bool,
    pub entries: Vec<VaultEntry>,
    pub error: Option<String>,
}

pub struct Vault {
    transport: Option<Box<dyn Transport>>,
    snapshot: Mutex<Snapshot>,
    remote: Option<Mutex<Option<String>>>,
}

struct Shared(Arc<dyn Transport>);

impl Transport for Shared {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.0.call(op, args)
    }
}

fn message(e: &ClientError) -> String {
    match e {
        ClientError::Api(api) => api.message.clone(),
        other => other.to_string(),
    }
}

struct Failed {
    line: String,
    api: Option<Box<ApiError>>,
}

impl Failed {
    fn line(line: String) -> Failed {
        Failed { line, api: None }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Vault {
    pub fn over(transport: Box<dyn Transport>) -> Vault {
        Vault {
            transport: Some(transport),
            snapshot: Mutex::new(Snapshot::default()),
            remote: None,
        }
    }

    pub fn remote_over(transport: Arc<dyn Transport>) -> Vault {
        Vault {
            transport: Some(Box::new(Shared(transport))),
            snapshot: Mutex::new(Snapshot::default()),
            remote: Some(Mutex::new(None)),
        }
    }

    pub fn remote_fixed(entries: Vec<VaultEntry>) -> Vault {
        let mut vault = Vault::fixed(entries);
        vault.remote = Some(Mutex::new(None));
        vault
    }

    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    pub fn set_node(&self, node: Option<&str>) {
        let Some(slot) = &self.remote else {
            return;
        };
        let mut held = lock(slot);
        let next = node.map(str::to_owned);
        if *held != next {
            *held = next;
            drop(held);
            if self.transport.is_some() {
                self.update(|s| *s = Snapshot::default());
            }
        }
    }

    pub fn fixed(entries: Vec<VaultEntry>) -> Vault {
        Vault {
            transport: None,
            snapshot: Mutex::new(Snapshot {
                attached: true,
                entries,
                error: None,
            }),
            remote: None,
        }
    }

    pub fn for_run(root: Option<&DataRoot>) -> Vault {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Vault::over(Box::new(Socket {
                socket: root.data_dir.join(SOCKET_NAME),
                version: env!("CARGO_PKG_VERSION").to_string(),
            })),
            None => Vault::fixed(Vec::new()),
        }
    }

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.snapshot).clone()
    }

    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut lock(&self.snapshot));
    }

    fn gate(&self, switches: &Switches) -> Result<(), String> {
        if self.transport.is_none() {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        }
        if let Some(slot) = &self.remote {
            return match *lock(slot) {
                Some(_) => Ok(()),
                None => Err(t::cluster::vault_unknown().to_owned()),
            };
        }
        match switches.message(Switch::Vault) {
            Some(why) => Err(why.to_owned()),
            None => Ok(()),
        }
    }

    fn current_node(&self) -> Option<String> {
        self.remote.as_ref().and_then(|slot| lock(slot).clone())
    }

    fn send(&self, switches: &Switches, call: &Call) -> Result<Value, String> {
        self.send_at(switches, call, None)
    }

    fn send_at(
        &self,
        switches: &Switches,
        call: &Call,
        pinned: Option<&str>,
    ) -> Result<Value, String> {
        self.exchange(switches, call, pinned)
            .map_err(|failed| failed.line)
    }

    fn exchange(
        &self,
        switches: &Switches,
        call: &Call,
        pinned: Option<&str>,
    ) -> Result<Value, Failed> {
        self.gate(switches).map_err(Failed::line)?;
        let Some(transport) = &self.transport else {
            return Err(Failed::line(crate::acting::DETACHED_MESSAGE.into()));
        };
        let mut args = call.args.clone();
        if self.remote.is_some() {
            let node = self.current_node().unwrap_or_default();
            if pinned.is_some_and(|p| p != node) {
                return Err(Failed::line(t::cluster::vault_moved().to_owned()));
            }
            args["node"] = Value::String(pinned.map_or(node, str::to_owned));
        }
        transport.call(call.op, args).map_err(|e| {
            if switches::detaches(&e) {
                self.update(|s| *s = Snapshot::default());
                return Failed::line(crate::acting::DETACHED_MESSAGE.to_owned());
            }
            let line = message(&e);
            self.update(|s| {
                s.attached = true;
                s.error = Some(line.clone());
            });
            Failed {
                line,
                api: match e {
                    ClientError::Api(api) => Some(Box::new(api)),
                    _ => None,
                },
            }
        })
    }

    pub fn refresh(&self, switches: &Switches) {
        if self.transport.is_none() {
            return;
        }
        if self.remote.is_none() && !switches.is_on(Switch::Vault) {
            self.update(|s| {
                s.attached = switches.attached;
                s.entries.clear();
                s.error = None;
            });
            return;
        }
        let asked = self.current_node();
        if let Ok(v) = self.send(switches, &Call::list()) {
            if self.current_node() != asked {
                return;
            }
            self.update(|s| {
                s.attached = true;
                s.entries = parse_entries(&v);
                s.error = None;
            });
        }
    }

    pub fn save(
        &self,
        switches: &Switches,
        entry: &VaultEntry,
        value: Option<&VaultValue>,
        replacing: Option<&str>,
        request: Option<&str>,
    ) -> Result<(), String> {
        self.save_at(None, switches, entry, value, (replacing, request))
    }

    pub fn save_at(
        &self,
        pinned: Option<&str>,
        switches: &Switches,
        entry: &VaultEntry,
        value: Option<&VaultValue>,
        (replacing, request): (Option<&str>, Option<&str>),
    ) -> Result<(), String> {
        let call = Call::save(entry, value, replacing, request);
        let v = self.send_at(switches, &call, pinned)?;
        self.kept(&v, replacing);
        Ok(())
    }

    fn kept(&self, reply: &Value, replacing: Option<&str>) {
        if let Ok(saved) = serde_json::from_value::<VaultEntry>(reply["entry"].clone()) {
            self.update(|s| {
                s.entries
                    .retain(|e| e.id() != saved.id() && Some(e.name.as_str()) != replacing);
                s.entries.push(saved);
                s.entries.sort_by_key(|e| e.name.to_lowercase());
                s.error = None;
            });
        }
    }

    pub fn save_for_consent(
        &self,
        switches: &Switches,
        entry: &VaultEntry,
        value: Option<&VaultValue>,
        replacing: Option<&str>,
        consent: &str,
    ) -> Result<(), Failure> {
        if self.remote.is_none() && !switches.is_on(Switch::Vault) {
            return Err(Failure::Refused(Refusal::VaultOff));
        }
        let call = Call::save(entry, value, replacing, Some(consent));
        match self.exchange(switches, &call, None) {
            Ok(v) => {
                self.kept(&v, replacing);
                Ok(())
            }
            Err(Failed { line, api: None }) => Err(Failure::Other(line)),
            Err(Failed { api: Some(api), .. }) => {
                let state = wants_state(&api)
                    .then(|| self.consent_state(switches, consent))
                    .flatten();
                Err(failure(&api, state.as_deref()))
            }
        }
    }

    fn consent_state(&self, switches: &Switches, id: &str) -> Option<String> {
        self.send(switches, &Call::consent_get(id))
            .ok()?
            .get("state")?
            .as_str()
            .map(str::to_owned)
    }

    pub fn remove(&self, switches: &Switches, name: &str) -> Result<(), String> {
        self.remove_at(None, switches, name)
    }

    pub fn remove_at(
        &self,
        pinned: Option<&str>,
        switches: &Switches,
        name: &str,
    ) -> Result<(), String> {
        self.send_at(switches, &Call::remove(name), pinned)?;
        self.update(|s| {
            s.entries.retain(|e| e.name != name);
            s.error = None;
        });
        Ok(())
    }

    pub fn answer(&self, switches: &Switches, id: &str, decision: Decision) -> Result<(), String> {
        self.send(switches, &Call::approval_answer(id, decision))
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::switches::VAULT_OFF;
    use notchling_engine::vault::entry::Kind;
    use notchling_local_api::{ApiError, ErrorCode};
    use serde_json::json;
    use std::sync::Arc;

    type Calls = Arc<Mutex<Vec<(String, Value)>>>;

    struct Fake {
        calls: Calls,
        fail: bool,
    }

    impl Transport for Fake {
        fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push((op.to_owned(), args.clone()));
            if self.fail {
                return Err(ClientError::Api(ApiError::new(
                    ErrorCode::Failed,
                    "the Keychain refused",
                )));
            }
            Ok(match op {
                "vault.list" => json!({"entries": [VaultEntry::new("npm", Kind::Token)]}),
                "vault.save" => {
                    json!({"entry": args["entry"], "answered": args.get("consentId").is_some()})
                }
                "vault.remove" => json!({"removed": true}),
                _ => json!({"answered": true}),
            })
        }
    }

    fn fake(fail: bool) -> (Vault, Calls) {
        let calls: Calls = Arc::default();
        let vault = Vault::over(Box::new(Fake {
            calls: calls.clone(),
            fail,
        }));
        (vault, calls)
    }

    #[test]
    fn off_sends_nothing_and_on_lists_saves_removes_and_answers() {
        let (vault, calls) = fake(false);
        assert!(vault.is_live());
        vault.refresh(&Switches::all_off());
        assert!(
            calls.lock().unwrap().is_empty(),
            "off: the daemon would refuse"
        );
        assert!(vault.snapshot().attached && vault.snapshot().entries.is_empty());
        let entry = VaultEntry::new("github", Kind::Token);
        assert_eq!(
            vault
                .save(&Switches::all_off(), &entry, None, None, None)
                .unwrap_err(),
            VAULT_OFF
        );
        assert_eq!(
            vault
                .answer(&Switches::detached(), "ap-1", Decision::Once)
                .unwrap_err(),
            crate::acting::DETACHED_MESSAGE
        );
        assert!(calls.lock().unwrap().is_empty());

        let on = Switches::all_on();
        vault.refresh(&on);
        assert_eq!(vault.snapshot().entries[0].name, "npm");
        let value = VaultValue {
            username: String::new(),
            secret: "s3cret".into(),
        };
        vault
            .save(&on, &entry, Some(&value), None, Some("r1"))
            .unwrap();
        let names: Vec<String> = vault
            .snapshot()
            .entries
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert_eq!(names, ["github", "npm"]);
        assert!(!format!("{:?}", vault.snapshot()).contains("s3cret"));
        vault.remove(&on, "npm").unwrap();
        assert_eq!(vault.snapshot().entries.len(), 1);
        vault.answer(&on, "ap-1", Decision::Always).unwrap();
        let sent: Vec<String> = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
        assert_eq!(
            sent,
            [
                "vault.list",
                "vault.save",
                "vault.remove",
                "vault.approval.answer"
            ]
        );
    }

    struct Refuses {
        calls: Calls,
        message: &'static str,
        reason: &'static str,
        state: &'static str,
    }

    impl Transport for Refuses {
        fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push((op.to_owned(), args.clone()));
            if op == "consent.get" {
                return Ok(json!({"id": args["id"], "state": self.state}));
            }
            Err(ClientError::Api(
                ApiError::new(ErrorCode::Forbidden, self.message).with_reason(self.reason),
            ))
        }
    }

    fn refused_with(message: &'static str, state: &'static str) -> (Vault, Calls) {
        let calls: Calls = Arc::default();
        let vault = Vault::over(Box::new(Refuses {
            calls: calls.clone(),
            message,
            reason: "needs_consent",
            state,
        }));
        (vault, calls)
    }

    #[test]
    fn a_save_under_a_consent_sends_its_id_and_nothing_else_about_it() {
        let (vault, calls) = fake(false);
        let entry = VaultEntry::new("github", Kind::Token);
        vault
            .save_for_consent(&Switches::all_on(), &entry, None, Some("gh"), "c1")
            .unwrap();
        let sent = calls.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "vault.save");
        assert_eq!(sent[0].1["consentId"], "c1");
        assert_eq!(sent[0].1["replacing"], "gh");
        assert!(sent[0].1.get("request").is_none());
        drop(sent);
        assert_eq!(vault.snapshot().entries[0].name, "github");
    }

    #[test]
    fn a_save_with_the_vault_off_is_refused_before_anything_is_sent() {
        let (vault, calls) = fake(false);
        let entry = VaultEntry::new("github", Kind::Token);
        let off = vault.save_for_consent(&Switches::all_off(), &entry, None, None, "c1");
        assert_eq!(off, Err(Failure::Refused(Refusal::VaultOff)));
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_refused_save_asks_the_daemon_what_became_of_the_request() {
        let entry = VaultEntry::new("github", Kind::Token);
        let on = Switches::all_on();
        let ended = "That request is no longer waiting for your yes.";
        let (vault, calls) = refused_with(ended, "expired");
        assert_eq!(
            vault.save_for_consent(&on, &entry, None, None, "c1"),
            Err(Failure::Refused(Refusal::Expired))
        );
        let sent: Vec<String> = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
        assert_eq!(sent, ["vault.save", "consent.get"]);
        let (vault, _) = refused_with(ended, "cancelled");
        assert_eq!(
            vault.save_for_consent(&on, &entry, None, None, "c1"),
            Err(Failure::Refused(Refusal::Withdrawn))
        );
        let drifted = "That request no longer matches what you were shown.";
        let (vault, _) = refused_with(drifted, "opened");
        assert_eq!(
            vault.save_for_consent(&on, &entry, None, None, "c1"),
            Err(Failure::Refused(Refusal::Changed))
        );
    }

    #[test]
    fn any_other_failure_of_a_save_keeps_the_daemons_words() {
        let (vault, calls) = fake(true);
        let entry = VaultEntry::new("github", Kind::Token);
        assert_eq!(
            vault.save_for_consent(&Switches::all_on(), &entry, None, None, "c1"),
            Err(Failure::Other("the Keychain refused".into()))
        );
        assert_eq!(calls.lock().unwrap().len(), 1, "no state read for it");
    }

    #[test]
    fn a_refusal_is_the_error_and_a_fixed_vault_never_sends() {
        let (vault, _) = fake(true);
        let on = Switches::all_on();
        vault.refresh(&on);
        assert_eq!(
            vault.snapshot().error.as_deref(),
            Some("the Keychain refused")
        );
        assert_eq!(vault.remove(&on, "x").unwrap_err(), "the Keychain refused");
        let fixed = Vault::fixed(vec![VaultEntry::new("npm", Kind::Token)]);
        assert!(!fixed.is_live());
        assert_eq!(fixed.snapshot().entries.len(), 1);
        assert!(fixed.remove(&on, "npm").is_err());
        fixed.refresh(&on);
        assert_eq!(
            fixed.snapshot().entries.len(),
            1,
            "a render keeps what it was given"
        );
    }

    #[test]
    fn a_remote_vault_forgets_its_list_when_the_node_changes_even_from_none() {
        let remote = Vault::remote_over(Arc::new(NoCalls));
        remote.update(|s| {
            s.attached = true;
            s.entries = vec![VaultEntry::new("old", Kind::Token)];
        });
        remote.set_node(Some("node-a"));
        assert!(remote.snapshot().entries.is_empty());
    }

    struct NoCalls;

    impl Transport for NoCalls {
        fn call(&self, _: &str, _: Value) -> Result<Value, ClientError> {
            Ok(Value::Null)
        }
    }

    #[test]
    fn a_pinned_node_goes_with_the_call_or_the_call_is_refused() {
        let calls: Arc<Mutex<Vec<Value>>> = Arc::default();
        struct Record(Arc<Mutex<Vec<Value>>>);
        impl Transport for Record {
            fn call(&self, _: &str, args: Value) -> Result<Value, ClientError> {
                self.0.lock().expect("calls").push(args);
                Ok(json!({"removed": true}))
            }
        }
        let remote = Vault::remote_over(Arc::new(Record(calls.clone())));
        let on = Switches::all_on();
        remote.set_node(Some("node-a"));
        remote.remove_at(Some("node-a"), &on, "x").expect("pinned");
        assert_eq!(calls.lock().expect("calls")[0]["node"], "node-a");
        remote.set_node(Some("node-b"));
        let entry = VaultEntry::new("x", Kind::Token);
        let refused = remote.save_at(Some("node-a"), &on, &entry, None, (None, None));
        assert!(refused.is_err());
        assert!(remote.remove_at(Some("node-a"), &on, "x").is_err());
        assert_eq!(
            calls.lock().expect("calls").len(),
            1,
            "nothing reached node-b"
        );
    }
}
