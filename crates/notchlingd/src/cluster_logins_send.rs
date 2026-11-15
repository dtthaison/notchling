use crate::cluster_logins::{LOGIN_DELIVER, LOGIN_ENTRY};
use crate::grant_broker::{Audit, Clock, Link, Release, Vault, View, LOG_DOWN};
use crate::grant_calls::Holder;
use crate::inbox_ops::InboxOps;
use crate::vault_ops::{ReleaseFor, VaultOps};
use notchling_engine::cluster::grants::decode_env;
use notchling_engine::cluster::login_files::{valid_token, CLAUDE};
use notchling_engine::cluster::{ClusterState, Member, MemberState, NodeId};
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use notchling_engine::vault::expiry::day_number;
use notchling_engine::vault::VaultEntry;
use notchling_engine::versions::Tool;
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

pub const DAY_MS: i64 = 86_400_000;
pub const NOTICE_DAYS: i64 = 14;
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

pub trait Notes: Send + Sync {
    fn raise(&self, item: Item);
    fn withdraw(&self, id: &str);
}

impl Notes for InboxOps {
    fn raise(&self, item: Item) {
        InboxOps::raise(self, item, None);
    }

    fn withdraw(&self, id: &str) {
        self.resolve(id, State::Withdrawn, None, None);
    }
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

    pub fn get(&self, node: &NodeId) -> Option<Stamp> {
        lock(&self.map).get(node.as_str()).cloned()
    }

    pub fn set(&self, node: &NodeId, stamp: Stamp) {
        let mut map = lock(&self.map);
        map.insert(node.as_str().to_string(), stamp);
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
    raised: BTreeSet<String>,
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

fn done_action() -> Vec<Action> {
    vec![Action {
        id: ActionId::Done,
        label: "Done".to_string(),
        primary: false,
    }]
}

fn notice(id: String, kind: Kind, text: (String, String), node: Option<&NodeId>, now: i64) -> Item {
    Item {
        id,
        kind,
        title: text.0,
        detail: text.1,
        source: Source {
            node: node.map(|n| n.to_string()),
            session: None,
            job: None,
        },
        created_ms: now,
        expires_ms: None,
        actions: done_action(),
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
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

    fn send(&self, member: &Member) -> Result<Stamp, String> {
        let who = ReleaseFor {
            node_name: member.name.clone(),
            job: format!("login:{CLAUDE}"),
            command: COMMAND.to_string(),
            folder: String::new(),
        };
        let names = vec![LOGIN_ENTRY.to_string()];
        let still = || self.still(&member.id);
        let released = self
            .vault
            .release(&who, &names, &still)
            .map_err(|e| e.message.clone())?;
        let refuse = |why: &str| {
            self.audit.refused(&member.id, &who, LOGIN_ENTRY);
            why.to_string()
        };
        let (entry, packed) = released
            .values
            .first()
            .ok_or_else(|| refuse("the vault answered with nothing"))?;
        let env = decode_env(packed).map_err(|why| refuse(&why))?;
        let token = env
            .get(&entry.secret_var)
            .filter(|v| valid_token(v.expose()))
            .ok_or_else(|| refuse("the login entry holds no token this machine can send"))?;
        let now = (self.clock)();
        let expires = expires_of(entry)
            .filter(|at| *at > now)
            .ok_or_else(|| refuse("the login entry has no expiry date in the future"))?;
        still().map_err(|why| refuse(&why))?;
        let release = Release {
            node: &member.id,
            who: &who,
            name: LOGIN_ENTRY,
            released: &released,
        };
        self.audit
            .released(&release)
            .map_err(|_| LOG_DOWN.to_string())?;
        still().map_err(|why| refuse(&why))?;
        let args = json!({"tool": CLAUDE, "value": token.expose(), "expiresMs": expires});
        self.link
            .call(&member.id, LOGIN_DELIVER, args)
            .map_err(|e| e.message.clone())?;
        Ok(Stamp {
            updated: entry.updated.clone(),
            expires: entry.expires.clone().unwrap_or_default(),
            sent_ms: now,
        })
    }

    fn settle(&self, wanted: BTreeMap<String, Item>) {
        let mut local = lock(&self.local);
        let stale: Vec<String> = local
            .raised
            .iter()
            .filter(|id| !wanted.contains_key(*id))
            .cloned()
            .collect();
        for id in stale {
            self.notes.withdraw(&id);
            local.raised.remove(&id);
        }
        for (id, item) in wanted {
            if local.raised.insert(id) {
                self.notes.raise(item);
            }
        }
    }

    fn notices(
        &self,
        entry: &VaultEntry,
        members: &[Member],
        expires: Option<i64>,
        now: i64,
    ) -> BTreeMap<String, Item> {
        let mut wanted = BTreeMap::new();
        match (expires, entry.expires.as_deref()) {
            (Some(at), Some(day)) if at.saturating_sub(now) <= NOTICE_DAYS * DAY_MS => {
                for m in members {
                    let id = format!("login-needed:{}:{day}", m.id);
                    let text = (
                        format!("Claude Code's token for {} expires on {day}", m.name),
                        format!(
                            "Renew {LOGIN_ENTRY} in the Secret Vault with a new expiry date; the cluster sends it again."
                        ),
                    );
                    wanted.insert(
                        id.clone(),
                        notice(id, Kind::LoginNeeded, text, Some(&m.id), now),
                    );
                }
            }
            (None, _) => {
                let id = format!("login-no-expiry:{}", entry.updated);
                let text = (
                    format!("{LOGIN_ENTRY} has no expiry date, so it isn't sent to Workers"),
                    "Set the day the token ends in the Secret Vault.".to_string(),
                );
                wanted.insert(id.clone(), notice(id, Kind::Notice, text, None, now));
            }
            _ => {}
        }
        wanted
    }

    pub fn reconcile(&self) {
        let _busy = lock(&self.busy);
        let (Some(state), Some(me)) = (self.view.state(), self.view.me()) else {
            return;
        };
        if state.vault != me || state.home != me {
            return;
        }
        let Ok(Some(entry)) = self.entries.entry(LOGIN_ENTRY) else {
            return;
        };
        let now = (self.clock)();
        let expires = expires_of(&entry);
        let members = eligible(&state, &me);
        let wanted = self.notices(&entry, &members, expires, now);
        self.settle(wanted);
        if expires.is_none_or(|at| at <= now) {
            return;
        }
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
                Ok(stamp) => {
                    lock(&self.local).holds.remove(member.id.as_str());
                    self.stamps.set(&member.id, stamp);
                }
                Err(_) => {
                    lock(&self.local)
                        .holds
                        .insert(member.id.as_str().to_string(), (key.clone(), now + HOLD_MS));
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
