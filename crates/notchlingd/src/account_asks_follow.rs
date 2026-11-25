use crate::acting_ops::Room;
use notchling_engine::consent::Consent;
use notchling_engine::organizations::Kind;
use notchling_local_api::topics;
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

pub const SIGN_IN_FOR_MS: i64 = 600_000;
pub const GRACE_MS: i64 = 30_000;
pub const SLOW_FOR_MS: i64 = 330_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Watch {
    ClaudeAdd {
        login: Option<String>,
        before: Vec<String>,
    },
    ProviderAdd {
        provider: String,
        attempt: Option<u64>,
        before: Vec<String>,
        limit_ms: i64,
    },
    Ticket(u64),
    ProviderSwitch {
        provider: String,
    },
    ProviderRemove {
        provider: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum End {
    Done(Value),
    Failed,
    Withdrawn,
}

#[derive(Clone)]
pub struct Entry {
    pub row: Consent,
    pub watch: Watch,
    pub started_ms: i64,
}

impl Entry {
    fn limit_ms(&self) -> i64 {
        match &self.watch {
            Watch::ClaudeAdd { .. } => SIGN_IN_FOR_MS,
            Watch::ProviderAdd { limit_ms, .. } => *limit_ms,
            _ => SLOW_FOR_MS - GRACE_MS,
        }
    }

    fn deadline_ms(&self) -> i64 {
        self.started_ms + self.limit_ms() + GRACE_MS
    }

    fn adds(&self) -> Option<&str> {
        match &self.watch {
            Watch::ClaudeAdd { .. } => Some("claude"),
            Watch::ProviderAdd { provider, .. } => Some(provider.as_str()),
            _ => None,
        }
    }

    fn failed_at(&self, now_ms: i64) -> End {
        if now_ms >= self.started_ms + self.limit_ms() {
            End::Withdrawn
        } else {
            End::Failed
        }
    }

    fn note(&mut self, topic: &str, event: &str, data: &Value) {
        let Watch::ClaudeAdd {
            login: Some(login),
            before,
        } = &mut self.watch
        else {
            return;
        };
        if topic != topics::ACCOUNTS
            || event != topics::ACCOUNTS_ADD_ENDED
            || data["state"] != "added"
            || data["login"].as_str() == Some(login.as_str())
        {
            return;
        }
        if let Some(raw) = data["account"]["id"].as_str() {
            let id = Kind::Claude.normal(raw);
            if !before.contains(&id) {
                before.push(id);
            }
        }
    }

    fn sees(&self, topic: &str, event: &str, data: &Value, now_ms: i64) -> Option<End> {
        let state = data["state"].as_str().unwrap_or_default();
        let same = |provider: &str, reason: &str| {
            topic == topics::PROVIDERS
                && event == topics::PROVIDERS_CHANGED
                && data["reason"] == reason
                && data["provider"] == provider
        };
        match &self.watch {
            Watch::ClaudeAdd { login, .. } => {
                if topic != topics::ACCOUNTS || event != topics::ACCOUNTS_ADD_ENDED {
                    return None;
                }
                if login
                    .as_deref()
                    .is_some_and(|l| data["login"].as_str() != Some(l))
                {
                    return None;
                }
                Some(match state {
                    "added" => End::Done(data.clone()),
                    "failed" => self.failed_at(now_ms),
                    _ => End::Failed,
                })
            }
            Watch::ProviderAdd {
                provider, attempt, ..
            } => {
                if !same(provider, "add")
                    || attempt.is_none()
                    || data["attempt"].as_u64() != *attempt
                {
                    return None;
                }
                match state {
                    "done" => Some(End::Done(Value::Null)),
                    "cancelled" => Some(End::Failed),
                    "failed" => Some(self.failed_at(now_ms)),
                    _ => None,
                }
            }
            Watch::Ticket(ticket) => {
                if topic != topics::ACCOUNTS
                    || event != topics::ACCOUNTS_DONE
                    || data["ticket"].as_u64() != Some(*ticket)
                {
                    return None;
                }
                Some(if data["ok"] == true {
                    End::Done(serde_json::json!({"result": data["result"]}))
                } else {
                    End::Failed
                })
            }
            Watch::ProviderSwitch { provider } => {
                if !same(provider, "switch") {
                    return None;
                }
                match state {
                    "done" => Some(End::Done(Value::Null)),
                    "failed" => Some(End::Failed),
                    _ => None,
                }
            }
            Watch::ProviderRemove { provider } => {
                same(provider, "remove").then_some(End::Done(Value::Null))
            }
        }
    }
}

pub type Settle = Arc<dyn Fn(Entry, End) + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

type Missed = Vec<(String, String, Value)>;

struct Held {
    id: String,
    adds: Option<String>,
    missed: Missed,
}

pub struct AccountFollow {
    entries: Mutex<Vec<Entry>>,
    held: Mutex<Vec<Held>>,
    settle: OnceLock<Settle>,
    clock: Clock,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AccountFollow {
    pub fn new(clock: Clock) -> Arc<AccountFollow> {
        Arc::new(AccountFollow {
            entries: Mutex::new(Vec::new()),
            held: Mutex::new(Vec::new()),
            settle: OnceLock::new(),
            clock,
        })
    }

    pub fn bind(&self, settle: Settle) {
        let _ = self.settle.set(settle);
    }

    pub fn start(&self, row: &Consent, watch: Watch, now_ms: i64) {
        let ended = {
            let mut entries = lock(&self.entries);
            let missed = {
                let mut held = lock(&self.held);
                let at = held.iter().position(|h| h.id == row.id);
                at.map(|at| held.remove(at).missed).unwrap_or_default()
            };
            entries.retain(|e| e.row.id != row.id);
            let mut entry = Entry {
                row: row.clone(),
                watch,
                started_ms: now_ms,
            };
            let mut end = None;
            for (topic, event, data) in missed {
                end = entry.sees(&topic, &event, &data, now_ms);
                if end.is_some() {
                    break;
                }
                entry.note(&topic, &event, &data);
            }
            match end {
                Some(end) => vec![(entry, end)],
                None => {
                    entries.push(entry);
                    Vec::new()
                }
            }
        };
        self.finish(ended);
    }

    pub fn hold(&self, id: &str, adds: Option<&str>) {
        let _entries = lock(&self.entries);
        let mut held = lock(&self.held);
        held.retain(|h| h.id != id);
        held.push(Held {
            id: id.to_string(),
            adds: adds.map(str::to_string),
            missed: Vec::new(),
        });
    }

    pub fn release(&self, id: &str) {
        let _entries = lock(&self.entries);
        lock(&self.held).retain(|h| h.id != id);
    }

    pub fn holding(&self) -> usize {
        lock(&self.held).len()
    }

    pub fn forget(&self, id: &str) {
        let mut entries = lock(&self.entries);
        entries.retain(|e| e.row.id != id);
        lock(&self.held).retain(|h| h.id != id);
    }

    pub fn adding(&self, tool: &str) -> bool {
        let entries = lock(&self.entries);
        entries.iter().any(|e| e.adds() == Some(tool))
            || lock(&self.held)
                .iter()
                .any(|h| h.adds.as_deref() == Some(tool))
    }

    pub fn len(&self) -> usize {
        lock(&self.entries).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn saw(&self, topic: &str, event: &str, data: &Value) {
        let now = (self.clock)();
        let ended: Vec<(Entry, End)> = {
            let mut entries = lock(&self.entries);
            if topic == topics::ACCOUNTS || topic == topics::PROVIDERS {
                for held in lock(&self.held).iter_mut() {
                    held.missed
                        .push((topic.to_string(), event.to_string(), data.clone()));
                }
            }
            let mut ended = Vec::new();
            let mut kept = Vec::new();
            for mut entry in entries.drain(..) {
                match entry.sees(topic, event, data, now) {
                    Some(end) if ended.is_empty() => ended.push((entry, end)),
                    _ => {
                        entry.note(topic, event, data);
                        kept.push(entry);
                    }
                }
            }
            *entries = kept;
            ended
        };
        self.finish(ended);
    }

    pub fn tick(&self, now_ms: i64) {
        let ended: Vec<(Entry, End)> = {
            let mut entries = lock(&self.entries);
            let (late, kept): (Vec<Entry>, Vec<Entry>) =
                entries.drain(..).partition(|e| now_ms >= e.deadline_ms());
            *entries = kept;
            late.into_iter().map(|e| (e, End::Withdrawn)).collect()
        };
        self.finish(ended);
    }

    fn finish(&self, ended: Vec<(Entry, End)>) {
        let Some(settle) = self.settle.get() else {
            return;
        };
        for (entry, end) in ended {
            settle(entry, end);
        }
    }
}

pub struct AccountTap {
    inner: Arc<dyn Room>,
    follow: Arc<AccountFollow>,
}

impl AccountTap {
    pub fn new(inner: Arc<dyn Room>, follow: Arc<AccountFollow>) -> Arc<AccountTap> {
        Arc::new(AccountTap { inner, follow })
    }
}

impl Room for AccountTap {
    fn publish(&self, topic: &str, event: &str, data: &Value) {
        self.inner.publish(topic, event, data);
        self.follow.saw(topic, event, data);
    }

    fn publish_to(&self, owner: &str, topic: &str, event: &str, data: &Value) {
        self.inner.publish_to(owner, topic, event, data);
    }

    fn can_answer(&self) -> bool {
        self.inner.can_answer()
    }

    fn can_follow(&self, topic: &str) -> bool {
        self.inner.can_follow(topic)
    }

    fn can_answer_prompts(&self) -> bool {
        self.inner.can_answer_prompts()
    }

    fn follow_prompts(&self, node: &str, lease_ms: u64) {
        self.inner.follow_prompts(node, lease_ms);
    }

    fn attach_prompt_push(&self, push: Arc<dyn crate::cluster_forward::PromptPush>) {
        self.inner.attach_prompt_push(push);
    }
}
