use std::sync::{Arc, Mutex};

use notchling_engine::vault::entry::{VaultEntry, VaultValue};
use notchling_local_api::{ClientError, SOCKET_NAME};
use notchling_locale as t;
use serde_json::Value;

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

    fn send(&self, switches: &Switches, call: &Call) -> Result<Value, String> {
        self.gate(switches)?;
        let Some(transport) = &self.transport else {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        };
        let mut args = call.args.clone();
        if let Some(slot) = &self.remote {
            let node = lock(slot).clone().unwrap_or_default();
            args["node"] = Value::String(node);
        }
        transport.call(call.op, args).map_err(|e| {
            if switches::detaches(&e) {
                self.update(|s| *s = Snapshot::default());
                return crate::acting::DETACHED_MESSAGE.to_owned();
            }
            let line = message(&e);
            self.update(|s| {
                s.attached = true;
                s.error = Some(line.clone());
            });
            line
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
        if let Ok(v) = self.send(switches, &Call::list()) {
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
        let v = self.send(switches, &Call::save(entry, value, replacing, request))?;
        if let Ok(saved) = serde_json::from_value::<VaultEntry>(v["entry"].clone()) {
            self.update(|s| {
                s.entries
                    .retain(|e| e.id() != saved.id() && Some(e.name.as_str()) != replacing);
                s.entries.push(saved);
                s.entries.sort_by_key(|e| e.name.to_lowercase());
                s.error = None;
            });
        }
        Ok(())
    }

    pub fn remove(&self, switches: &Switches, name: &str) -> Result<(), String> {
        self.send(switches, &Call::remove(name))?;
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

    pub fn cancel_request(&self, switches: &Switches, id: &str) -> Result<(), String> {
        self.send(switches, &Call::request_cancel(id)).map(|_| ())
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
                    json!({"entry": args["entry"], "answered": args.get("request").is_some()})
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
        vault.cancel_request(&on, "r1").unwrap();
        let sent: Vec<String> = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
        assert_eq!(
            sent,
            [
                "vault.list",
                "vault.save",
                "vault.remove",
                "vault.approval.answer",
                "vault.request.cancel"
            ]
        );
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
        let (remote, _) = (Vault::remote_over(Arc::new(Shared(Arc::new(NoCalls)))), ());
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
}
