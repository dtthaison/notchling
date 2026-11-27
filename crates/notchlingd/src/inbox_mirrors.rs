use notchling_engine::cluster::{qualify, NodeId, Qualified};
use notchling_engine::inbox::{
    clean_label, clean_note, clean_place, peer_alert_actions, ActionId, Change, Item, Kind, Place,
    State, DETAIL_MAX, DONE_KEEP_MS, NOTE_MAX, TITLE_MAX,
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
    fn answer_noted(
        &self,
        owner: &NodeId,
        id: &str,
        action: ActionId,
        by: &str,
        _note: Option<&str>,
    ) -> Result<Value, ApiError> {
        self.answer(owner, id, action, by)
    }
    fn home_items(&self) -> Home;

    fn machine_name(&self, _node: &NodeId) -> Option<String> {
        None
    }
}

pub fn name_machines(items: &mut [Item], link: &dyn InboxLink) {
    for item in items {
        let Some(node) = item.source.node.clone().and_then(|n| NodeId::parse(&n)) else {
            continue;
        };
        let Some(name) = link.machine_name(&node) else {
            continue;
        };
        let place = item.place.get_or_insert_with(Place::default);
        place.machine = Some(node.to_string());
        place.machine_name = clean_label(&name, SOURCE_MAX);
    }
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

pub fn display(owner: &NodeId, item: Item) -> Result<Item, Refusal> {
    display_named(owner, item, None)
}

pub fn display_named(
    owner: &NodeId,
    mut item: Item,
    machine_name: Option<String>,
) -> Result<Item, Refusal> {
    bare_id(owner, &item.id)?;
    item.id = qualify(owner, &item.id);
    Ok(sanitize(owner, item, machine_name))
}

fn sanitize(owner: &NodeId, mut item: Item, machine_name: Option<String>) -> Item {
    item.source.node = Some(owner.to_string());
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
    for field in [&mut source.session, &mut source.job] {
        *field = field.take().map(|text| cut(text, SOURCE_MAX));
    }
    item.answered_by = item.answered_by.take().map(|by| cut(by, BY_MAX));
    item.note = item
        .note
        .take()
        .and_then(|note| clean_note(&note, NOTE_MAX));
    if let Some(outcome) = &mut item.outcome {
        outcome.reason = outcome
            .reason
            .take()
            .filter(|reason| crate::consent_failure::is_known_line(reason));
    }
    item.alert_actions = peer_alert_actions(item.kind, &item.actions);
    if item.state == State::Open
        && item.blocking == Some(true)
        && item.waiter.is_some()
        && item.waiting_since.is_some()
    {
        let now = crate::acting_ops::now_ms();
        item.waiting_since = item.waiting_since.map(|since| since.min(now));
    } else {
        item.clear_live();
    }
    let place = item.place.get_or_insert_with(Place::default);
    place.machine = Some(owner.to_string());
    place.machine_name = machine_name.and_then(|name| clean_label(&name, SOURCE_MAX));
    place.session = place
        .session
        .take()
        .and_then(|text| clean_place(&text, SOURCE_MAX, false, true));
    place.session_name = place
        .session_name
        .take()
        .and_then(|text| clean_place(&text, SOURCE_MAX, false, false));
    place.branch = place
        .branch
        .take()
        .and_then(|text| clean_place(&text, SOURCE_MAX, true, false));
    item
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
        self.changed_named(owner, item, None)
    }

    pub fn changed_named(
        &mut self,
        owner: &NodeId,
        item: Item,
        machine_name: Option<String>,
    ) -> Result<Vec<Change>, Refusal> {
        let shown = display_named(owner, item, machine_name)?;
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
        self.sync_named(owner, open, None)
    }

    pub fn sync_named(
        &mut self,
        owner: &NodeId,
        open: Vec<Item>,
        machine_name: Option<String>,
    ) -> Result<Vec<Change>, Refusal> {
        let mut shown = Vec::new();
        for item in open.into_iter().filter(|item| item.state == State::Open) {
            shown.push(display_named(owner, item, machine_name.clone())?);
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
            let owner = match Qualified::parse(&item.id) {
                Some(q) if q.node == *me => return None,
                Some(q) => q.node,
                None => {
                    item.id = qualify(home, &item.id);
                    home.clone()
                }
            };
            Some(sanitize(&owner, item, None))
        })
        .collect()
}
