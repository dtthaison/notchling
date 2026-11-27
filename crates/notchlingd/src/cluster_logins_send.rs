use crate::cluster_logins::{LOGIN_DELIVER, LOGIN_ENTRY, LOGIN_REVOKE};
pub use crate::cluster_logins_notes::DAY_MS;
use crate::cluster_logins_notes::{settle, wanted, Notes};
use crate::grant_broker::{Audit, Clock, Link, Release, Vault, View};
use crate::grant_calls::Holder;
use crate::vault_ops::{was_denied, ReleaseFor, VaultOps};
use notchling_engine::cluster::grants::decode_env;
use notchling_engine::cluster::login_files::{valid_token, CLAUDE};
use notchling_engine::cluster::{ClusterState, Member, MemberState, NodeId};
use notchling_engine::vault::expiry::day_number;
use notchling_engine::vault::VaultEntry;
use notchling_engine::versions::Tool;
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

pub const HOLD_MS: i64 = 10 * 60_000;
pub const RESEND_MS: i64 = 10 * 60_000;
const TICK: Duration = Duration::from_secs(10);
const STAMPS_FILE: &str = "login-sent.json";
const COMMAND: &str = "Claude Code's login for jobs (CLAUDE_CODE_OAUTH_TOKEN)";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub trait Entries: Send + Sync {
    fn entry(&self, name: &str) -> Result<Option<VaultEntry>, ApiError>;
}

impl Entries for VaultOps {
    fn entry(&self, name: &str) -> Result<Option<VaultEntry>, ApiError> {
        self.info(name)
    }
}

pub trait Online: Send + Sync {
    fn online(&self, node: &NodeId) -> bool;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub updated: String,
    pub expires: String,
    pub sent_ms: i64,
}

fn stamp_json(stamp: &Stamp) -> Value {
    json!({"updated": stamp.updated, "expires": stamp.expires, "sentMs": stamp.sent_ms})
}

fn stamp_from(value: &Value) -> Option<Stamp> {
    Some(Stamp {
        updated: value.get("updated")?.as_str()?.to_string(),
        expires: value.get("expires")?.as_str()?.to_string(),
        sent_ms: value.get("sentMs")?.as_i64()?,
    })
}

fn stamps_from(text: &str) -> BTreeMap<String, Stamp> {
    let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    parsed
        .get("sent")
        .and_then(Value::as_object)
        .map(|sent| {
            sent.iter()
                .filter_map(|(node, v)| Some((node.clone(), stamp_from(v)?)))
                .collect()
        })
        .unwrap_or_default()
}

pub struct Stamps {
    path: Option<PathBuf>,
    map: Mutex<BTreeMap<String, Stamp>>,
}

impl Stamps {
    pub fn in_memory() -> Stamps {
        Stamps {
            path: None,
            map: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn load(cluster_dir: &std::path::Path) -> Stamps {
        let path = cluster_dir.join(STAMPS_FILE);
        let map = std::fs::read_to_string(&path)
            .map(|text| stamps_from(&text))
            .unwrap_or_default();
        Stamps {
            path: Some(path),
            map: Mutex::new(map),
        }
    }

    pub fn nodes(&self) -> Vec<NodeId> {
        lock(&self.map)
            .keys()
            .filter_map(|id| NodeId::parse(id))
            .collect()
    }

    pub fn remove(&self, node: &NodeId) {
        let mut map = lock(&self.map);
        if map.remove(node.as_str()).is_some() {
            self.save(&map);
        }
    }

    pub fn get(&self, node: &NodeId) -> Option<Stamp> {
        lock(&self.map).get(node.as_str()).cloned()
    }

    pub fn set(&self, node: &NodeId, stamp: Stamp) {
        let mut map = lock(&self.map);
        map.insert(node.as_str().to_string(), stamp);
        self.save(&map);
    }

    fn save(&self, map: &BTreeMap<String, Stamp>) {
        let Some(path) = &self.path else {
            return;
        };
        let sent: serde_json::Map<String, Value> = map
            .iter()
            .map(|(node, stamp)| (node.clone(), stamp_json(stamp)))
            .collect();
        if let Ok(bytes) = serde_json::to_vec(&json!({"sent": sent})) {
            let _ = notchling_platform::files::write_private_no_follow(path, &bytes);
        }
    }
}

#[derive(Default)]
struct Local {
    holds: BTreeMap<String, (String, i64)>,
    revoke_holds: BTreeMap<String, i64>,
}

enum Fail {
    Denied,
    Other,
}

pub struct LoginDeliverer {
    vault: Arc<dyn Vault>,
    entries: Arc<dyn Entries>,
    link: Arc<dyn Link>,
    audit: Arc<dyn Audit>,
    view: Arc<dyn View>,
    online: Arc<dyn Online>,
    notes: Arc<dyn Notes>,
    clock: Clock,
    stamps: Stamps,
    local: Mutex<Local>,
    busy: Mutex<()>,
    nudged: Mutex<bool>,
    wake: Condvar,
    stop: AtomicBool,
}

pub struct Parts {
    pub vault: Arc<dyn Vault>,
    pub entries: Arc<dyn Entries>,
    pub link: Arc<dyn Link>,
    pub audit: Arc<dyn Audit>,
    pub view: Arc<dyn View>,
    pub online: Arc<dyn Online>,
    pub notes: Arc<dyn Notes>,
}

pub fn expires_of(entry: &VaultEntry) -> Option<i64> {
    let day = day_number(entry.expires.as_deref()?)?;
    day.checked_mul(DAY_MS)
}

fn has_login(member: &Member) -> bool {
    member
        .versions
        .as_ref()
        .and_then(|v| v.logins.get(&Tool::Claude))
        .copied()
        .unwrap_or(false)
}

fn eligible(state: &ClusterState, me: &NodeId) -> Vec<Member> {
    state
        .members
        .values()
        .filter(|m| {
            m.id != *me
                && m.state == MemberState::Active
                && m.secrets.allows(LOGIN_ENTRY)
                && m.versions
                    .as_ref()
                    .is_some_and(|v| v.tools.contains_key(&Tool::Claude))
        })
        .cloned()
        .collect()
}

fn due(stamp: Option<&Stamp>, entry: &VaultEntry, member: &Member, now: i64) -> bool {
    match stamp {
        None => true,
        Some(s) => {
            s.updated != entry.updated
                || Some(&s.expires) != entry.expires.as_ref()
                || (!has_login(member) && now.saturating_sub(s.sent_ms) >= RESEND_MS)
        }
    }
}

impl LoginDeliverer {
    pub fn new(parts: Parts, stamps: Stamps, clock: Clock) -> Arc<LoginDeliverer> {
        Arc::new(LoginDeliverer {
            vault: parts.vault,
            entries: parts.entries,
            link: parts.link,
            audit: parts.audit,
            view: parts.view,
            online: parts.online,
            notes: parts.notes,
            clock,
            stamps,
            local: Mutex::new(Local::default()),
            busy: Mutex::new(()),
            nudged: Mutex::new(false),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
        })
    }

    fn still(&self, node: &NodeId) -> Result<(), String> {
        let state = self.view.state().ok_or("this node is in no cluster")?;
        let me = self.view.me().ok_or("this node is in no cluster")?;
        if state.vault != me || state.home != me {
            return Err("this machine isn't the primary vault node now".to_string());
        }
        let member = state
            .members
            .get(node)
            .filter(|m| m.state == MemberState::Active)
            .ok_or("that machine isn't an active member")?;
        if !member.secrets.allows(LOGIN_ENTRY) {
            return Err(format!("the login isn't allowed on {} now", member.name));
        }
        Ok(())
    }

    fn send(&self, member: &Member) -> Result<(), Fail> {
        let who = ReleaseFor {
            node_name: member.name.clone(),
            job: format!("login:{CLAUDE}"),
            command: COMMAND.to_string(),
            folder: String::new(),
        };
        let names = vec![LOGIN_ENTRY.to_string()];
        let still = || self.still(&member.id);
        let released = self.vault.release(&who, &names, &still).map_err(|e| {
            if was_denied(&e) {
                Fail::Denied
            } else {
                Fail::Other
            }
        })?;
        let refuse = || {
            self.audit.refused(&member.id, &who, LOGIN_ENTRY);
            Fail::Other
        };
        let (entry, packed) = released.values.first().ok_or_else(refuse)?;
        let env = decode_env(packed).map_err(|_| refuse())?;
        let token = env
            .get(&entry.secret_var)
            .filter(|v| valid_token(v.expose()))
            .ok_or_else(refuse)?;
        let now = (self.clock)();
        let expires = expires_of(entry)
            .filter(|at| *at > now)
            .ok_or_else(refuse)?;
        still().map_err(|_| refuse())?;
        let release = Release {
            node: &member.id,
            who: &who,
            name: LOGIN_ENTRY,
            released: &released,
        };
        self.audit.released(&release).map_err(|_| Fail::Other)?;
        still().map_err(|_| refuse())?;
        let args = json!({"tool": CLAUDE, "value": token.expose(), "expiresMs": expires});
        let stamp = Stamp {
            updated: entry.updated.clone(),
            expires: entry.expires.clone().unwrap_or_default(),
            sent_ms: now,
        };
        self.stamps.set(&member.id, stamp);
        self.link
            .call(&member.id, LOGIN_DELIVER, args)
            .map_err(|_| Fail::Other)?;
        Ok(())
    }

    fn revoke_stale(&self, state: &ClusterState, sendable: bool, now: i64) {
        for node in self.stamps.nodes() {
            let member = state
                .members
                .get(&node)
                .filter(|m| m.state == MemberState::Active);
            let Some(member) = member else {
                self.stamps.remove(&node);
                continue;
            };
            let keep = sendable && member.secrets.allows(LOGIN_ENTRY);
            let held = lock(&self.local)
                .revoke_holds
                .get(node.as_str())
                .is_some_and(|until| now < *until);
            if keep || held || !self.online.online(&node) {
                continue;
            }
            if self.link.call(&node, LOGIN_REVOKE, json!({})).is_ok() {
                lock(&self.local).revoke_holds.remove(node.as_str());
                self.stamps.remove(&node);
            } else {
                lock(&self.local)
                    .revoke_holds
                    .insert(node.as_str().to_string(), now + HOLD_MS);
            }
        }
    }

    pub fn reconcile(&self) {
        let _busy = lock(&self.busy);
        let (Some(state), Some(me)) = (self.view.state(), self.view.me()) else {
            return;
        };
        if state.vault != me || state.home != me {
            return;
        }
        let Ok(found) = self.entries.entry(LOGIN_ENTRY) else {
            return;
        };
        let now = (self.clock)();
        let expires = found.as_ref().and_then(expires_of);
        let members = eligible(&state, &me);
        let notes = match &found {
            Some(entry) => wanted(entry, &members, expires, now),
            None => BTreeMap::new(),
        };
        settle(self.notes.as_ref(), notes, found.as_ref());
        let sendable = found.filter(|_| expires.is_some_and(|at| at > now));
        self.revoke_stale(&state, sendable.is_some(), now);
        let Some(entry) = sendable else {
            return;
        };
        let key = format!(
            "{}|{}",
            entry.updated,
            entry.expires.clone().unwrap_or_default()
        );
        for member in &members {
            if !self.online.online(&member.id) {
                continue;
            }
            let held = lock(&self.local)
                .holds
                .get(member.id.as_str())
                .is_some_and(|(k, until)| *k == key && now < *until);
            let stamp = self.stamps.get(&member.id);
            if held || !due(stamp.as_ref(), &entry, member, now) {
                continue;
            }
            match self.send(member) {
                Ok(()) => {
                    lock(&self.local).holds.remove(member.id.as_str());
                }
                Err(fail) => {
                    let until = match fail {
                        Fail::Denied => i64::MAX,
                        Fail::Other => now + HOLD_MS,
                    };
                    lock(&self.local)
                        .holds
                        .insert(member.id.as_str().to_string(), (key.clone(), until));
                }
            }
        }
    }

    pub fn nudge(&self) {
        *lock(&self.nudged) = true;
        self.wake.notify_all();
    }

    fn wait(&self) {
        let mut nudged = lock(&self.nudged);
        if !*nudged {
            nudged = self
                .wake
                .wait_timeout(nudged, TICK)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *nudged = false;
    }

    pub fn spawn(self: &Arc<LoginDeliverer>) {
        let deliverer = self.clone();
        let spawned = std::thread::Builder::new()
            .name("login-deliverer".into())
            .spawn(move || {
                while !deliverer.stop.load(Ordering::SeqCst) {
                    deliverer.reconcile();
                    deliverer.wait();
                }
            });
        if spawned.is_err() {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.nudge();
    }
}

pub struct NodeOnline {
    holder: Holder,
}

impl NodeOnline {
    pub fn new(holder: Holder) -> NodeOnline {
        NodeOnline { holder }
    }
}

impl Online for NodeOnline {
    fn online(&self, node: &NodeId) -> bool {
        self.holder
            .get()
            .is_some_and(|this| flag(&this.status(), node))
    }
}

pub fn flag(status: &Value, node: &NodeId) -> bool {
    status
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|m| m.get("id").and_then(Value::as_str) == Some(node.as_str()))
        .and_then(|m| m.get("online").and_then(Value::as_bool))
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "cluster_logins_send_tests.rs"]
mod tests;
