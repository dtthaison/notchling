use notchling_engine::cluster::{qualify, NodeId, Qualified};
use notchling_engine::inbox::{
    ActionId, Change, Item, Kind, State, DETAIL_MAX, DONE_KEEP_MS, TITLE_MAX,
};
use notchling_local_api::ApiError;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub type Tap = Arc<dyn Fn(Vec<Change>) + Send + Sync>;

pub trait InboxLink: Send + Sync {
    fn me(&self) -> Option<NodeId>;
    fn answer(
        &self,
        owner: &NodeId,
        id: &str,
        action: ActionId,
        by: &str,
    ) -> Result<Value, ApiError>;
    fn home_items(&self) -> Home;
}

pub enum Home {
    Off,
    Unreachable,
    Items(NodeId, Vec<Item>),
}

pub const MAX_PER_NODE: usize = 300;
pub const ID_MAX: usize = 200;
pub const ACTIONS_MAX: usize = 8;
pub const LABEL_MAX: usize = 60;
pub const SOURCE_MAX: usize = 200;
pub const BY_MAX: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Qualified,
    Foreign,
    Empty,
    TooLong,
    Full,
}

impl Refusal {
    pub fn text(&self) -> &'static str {
        match self {
            Refusal::Qualified => "an item id comes unqualified from its own node",
            Refusal::Foreign => "an item can only come from the node that owns it",
            Refusal::Empty => "an item needs an id",
            Refusal::TooLong => "an item id is too long",
            Refusal::Full => "this node holds too many items from that node",
        }
    }
}

fn cut(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

pub fn bare_id(owner: &NodeId, id: &str) -> Result<(), Refusal> {
    if id.is_empty() {
        return Err(Refusal::Empty);
    }
    if id.chars().count() > ID_MAX {
        return Err(Refusal::TooLong);
    }
    match Qualified::parse(id) {
        Some(q) if q.node != *owner => Err(Refusal::Foreign),
        Some(_) => Err(Refusal::Qualified),
        None => Ok(()),
    }
}

pub fn offered(kind: Kind) -> &'static [ActionId] {
    use ActionId::*;
    match kind {
        Kind::SecretApproval => &[Allow, AllowOnce, Deny],
        Kind::SecretRequest | Kind::SettingChange | Kind::Consent => &[Allow, Deny],
        Kind::LimitNotice
        | Kind::LoginNeeded
        | Kind::ActionFailed
        | Kind::Failover
        | Kind::Notice => &[Done],
        Kind::PausedStep => &[Continue, Done],
        Kind::OutboxUnsent => &[Send, Discard],
        Kind::PrimaryBack => &[Keep],
        Kind::SessionWaits => &[WaitFor],
        Kind::Question => &[Cancel, Choice1, Choice2, Choice3, Choice4, Choice5, Choice6],
        Kind::NodeJoin
        | Kind::VersionCheckFailed
        | Kind::VersionDrift
        | Kind::PrimaryUnreachable
        | Kind::NoPrimary
        | Kind::ShellStuck => &[],
    }
}

pub fn display(owner: &NodeId, mut item: Item) -> Result<Item, Refusal> {
    bare_id(owner, &item.id)?;
    item.id = qualify(owner, &item.id);
    if item.source.node.is_none() {
        item.source.node = Some(owner.to_string());
    }
    item.open = None;
    let allowed = offered(item.kind);
    item.actions.retain(|action| allowed.contains(&action.id));
    item.switch = item.switch.filter(|name| {
        item.kind == Kind::Consent && notchling_engine::cluster::policy::cluster_may_set(name)
    });
    item.title = cut(item.title, TITLE_MAX);
    item.detail = cut(item.detail, DETAIL_MAX);
    item.actions.truncate(ACTIONS_MAX);
    for action in &mut item.actions {
        action.label = cut(std::mem::take(&mut action.label), LABEL_MAX);
    }
    let source = &mut item.source;
    for field in [&mut source.node, &mut source.session, &mut source.job] {
        *field = field.take().map(|text| cut(text, SOURCE_MAX));
    }
    item.answered_by = item.answered_by.take().map(|by| cut(by, BY_MAX));
    Ok(item)
}

#[derive(Default)]
pub struct Mirrors {
    items: BTreeMap<String, Item>,
}

fn owned_by(owner: &NodeId) -> String {
    format!("{owner}/")
}

impl Mirrors {
    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.get(id)
    }

    pub fn items(&self) -> Vec<Item> {
        self.items.values().cloned().collect()
    }

    pub fn unread(&self) -> usize {
        self.items
            .values()
            .filter(|item| item.state == State::Open && !item.seen)
            .count()
    }

    fn count(&self, owner: &NodeId) -> usize {
        let prefix = owned_by(owner);
        self.items
            .keys()
            .filter(|id| id.starts_with(&prefix))
            .count()
    }

    fn evict_one(&mut self, owner: &NodeId) -> Option<Change> {
        let prefix = owned_by(owner);
        let oldest = self
            .items
            .values()
            .filter(|item| item.id.starts_with(&prefix) && item.state != State::Open)
            .min_by_key(|item| item.closed_ms.unwrap_or(item.created_ms))
            .map(|item| item.id.clone())?;
        self.items.remove(&oldest);
        Some(Change::Removed(oldest))
    }

    pub fn changed(&mut self, owner: &NodeId, item: Item) -> Result<Vec<Change>, Refusal> {
        let shown = display(owner, item)?;
        let mut changes = Vec::new();
        if !self.items.contains_key(&shown.id) && self.count(owner) >= MAX_PER_NODE {
            changes.extend(self.evict_one(owner));
            if self.count(owner) >= MAX_PER_NODE {
                return Err(Refusal::Full);
            }
        }
        changes.extend(self.put(shown));
        Ok(changes)
    }

    pub fn removed(&mut self, owner: &NodeId, id: &str) -> Result<Option<Change>, Refusal> {
        bare_id(owner, id)?;
        let id = qualify(owner, id);
        Ok(self.items.remove(&id).map(|_| Change::Removed(id)))
    }

    pub fn sync(&mut self, owner: &NodeId, open: Vec<Item>) -> Result<Vec<Change>, Refusal> {
        let mut shown = Vec::new();
        for item in open.into_iter().filter(|item| item.state == State::Open) {
            shown.push(display(owner, item)?);
        }
        shown.truncate(MAX_PER_NODE);
        let mut changes = Vec::new();
        let prefix = owned_by(owner);
        let stale: Vec<String> = self
            .items
            .values()
            .filter(|item| item.id.starts_with(&prefix) && item.state == State::Open)
            .filter(|item| !shown.iter().any(|new| new.id == item.id))
            .map(|item| item.id.clone())
            .collect();
        for id in stale {
            self.items.remove(&id);
            changes.push(Change::Removed(id));
        }
        for item in shown {
            changes.extend(self.put(item));
        }
        Ok(changes)
    }

    fn put(&mut self, mut shown: Item) -> Option<Change> {
        if let Some(old) = self.items.get(&shown.id) {
            shown.seen = shown.seen || old.seen;
            if *old == shown {
                return None;
            }
        }
        let known = self.items.insert(shown.id.clone(), shown.clone());
        Some(match known {
            Some(_) => Change::Updated(shown),
            None => Change::Raised(shown),
        })
    }

    pub fn mark_seen(&mut self, ids: &[String]) -> Vec<Change> {
        let mut changes = Vec::new();
        for item in self.items.values_mut() {
            if !item.seen && ids.contains(&item.id) {
                item.seen = true;
                changes.push(Change::Updated(item.clone()));
            }
        }
        changes
    }

    pub fn tick(&mut self, now_ms: i64) -> Vec<Change> {
        let gone: Vec<String> = self
            .items
            .values()
            .filter(|item| item.state != State::Open)
            .filter(|item| now_ms - item.closed_ms.unwrap_or(item.created_ms) > DONE_KEEP_MS)
            .map(|item| item.id.clone())
            .collect();
        let mut changes = Vec::new();
        for id in gone {
            self.items.remove(&id);
            changes.push(Change::Removed(id));
        }
        changes
    }

    pub fn keep_owners(&mut self, owners: &BTreeSet<NodeId>) -> Vec<Change> {
        let gone: Vec<String> = self
            .items
            .keys()
            .filter(|id| !Qualified::parse(id).is_some_and(|q| owners.contains(&q.node)))
            .cloned()
            .collect();
        let mut changes = Vec::new();
        for id in gone {
            self.items.remove(&id);
            changes.push(Change::Removed(id));
        }
        changes
    }

    pub fn clear(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.items)
            .into_keys()
            .map(Change::Removed)
            .collect()
    }
}

pub fn localize(me: &NodeId, home: &NodeId, items: Vec<Item>) -> Vec<Item> {
    items
        .into_iter()
        .filter_map(|mut item| {
            match Qualified::parse(&item.id) {
                Some(q) if q.node == *me => return None,
                Some(_) => {}
                None => {
                    item.id = qualify(home, &item.id);
                    if item.source.node.is_none() {
                        item.source.node = Some(home.to_string());
                    }
                }
            }
            item.open = None;
            Some(item)
        })
        .collect()
}
