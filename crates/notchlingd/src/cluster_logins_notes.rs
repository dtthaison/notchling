use crate::inbox_ops::InboxOps;
use notchling_engine::cluster::{Member, NodeId};
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use notchling_engine::vault::VaultEntry;
use std::collections::BTreeMap;

pub const NEEDED: &str = "login-needed:";
pub const NO_EXPIRY: &str = "login-no-expiry:";
pub const NOTICE_DAYS: i64 = 14;
pub const DAY_MS: i64 = 86_400_000;
pub(crate) const VARIANTS: usize = 30;

pub trait Notes: Send + Sync {
    fn raise(&self, item: Item);
    fn withdraw(&self, id: &str);
    fn state(&self, id: &str) -> Option<State>;
    fn open_ids(&self) -> Vec<String>;
}

impl Notes for InboxOps {
    fn raise(&self, item: Item) {
        InboxOps::raise(self, item, None);
    }

    fn withdraw(&self, id: &str) {
        self.resolve(id, State::Withdrawn, None, None);
    }

    fn state(&self, id: &str) -> Option<State> {
        self.items()
            .into_iter()
            .find(|i| i.id == id)
            .map(|i| i.state)
    }

    fn open_ids(&self) -> Vec<String> {
        self.items()
            .into_iter()
            .filter(|i| i.state == State::Open)
            .map(|i| i.id)
            .collect()
    }
}

pub(crate) fn notice(
    id: String,
    kind: Kind,
    text: (String, String),
    node: Option<&NodeId>,
    now: i64,
) -> Item {
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
        actions: vec![Action {
            id: ActionId::Done,
            label: "Done".to_string(),
            primary: false,
        }],
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
        switch: None,
        ..Default::default()
    }
}

pub fn wanted(
    entry: &VaultEntry,
    members: &[Member],
    expires: Option<i64>,
    now: i64,
) -> BTreeMap<String, Item> {
    let mut wanted = BTreeMap::new();
    match (expires, entry.expires.as_deref()) {
        (Some(at), Some(day)) if at.saturating_sub(now) <= NOTICE_DAYS * DAY_MS => {
            for m in members {
                let id = format!("{NEEDED}{}:{day}", m.id);
                let text = (
                    format!("Claude Code's token for {} expires on {day}", m.name),
                    "Renew claude-worker in the Secret Vault with a new expiry date; the cluster sends it again."
                        .to_string(),
                );
                wanted.insert(
                    id.clone(),
                    notice(id, Kind::LoginNeeded, text, Some(&m.id), now),
                );
            }
        }
        (None, _) if !members.is_empty() => {
            let id = format!("{NO_EXPIRY}{}", entry.updated);
            let text = (
                "claude-worker has no expiry date, so it isn't sent to Workers".to_string(),
                "Set the day the token ends in the Secret Vault.".to_string(),
            );
            wanted.insert(id.clone(), notice(id, Kind::Notice, text, None, now));
        }
        _ => {}
    }
    wanted
}

pub(crate) fn base_of(id: &str) -> &str {
    id.split('#').next().unwrap_or(id)
}

fn ours(base: &str) -> bool {
    base.starts_with(NEEDED) || base.starts_with(NO_EXPIRY)
}

fn current(base: &str, entry: &VaultEntry) -> bool {
    let needed = entry
        .expires
        .as_deref()
        .is_some_and(|day| base.starts_with(NEEDED) && base.ends_with(&format!(":{day}")));
    needed || base == format!("{NO_EXPIRY}{}", entry.updated)
}

fn variant(base: &str, n: usize) -> String {
    if n == 1 {
        base.to_string()
    } else {
        format!("{base}#{n}")
    }
}

pub(crate) fn raise_once(notes: &dyn Notes, base: &str, mut item: Item) {
    let mut latest = None;
    for n in 1..=VARIANTS {
        let id = variant(base, n);
        match notes.state(&id) {
            Some(state) => latest = Some(state),
            None => {
                if matches!(latest, None | Some(State::Withdrawn | State::Expired)) {
                    item.id = id;
                    notes.raise(item);
                }
                return;
            }
        }
    }
}

pub fn settle(notes: &dyn Notes, wanted: BTreeMap<String, Item>, entry: Option<&VaultEntry>) {
    for id in notes.open_ids() {
        let base = base_of(&id);
        if ours(base) && !entry.is_some_and(|e| current(base, e)) {
            notes.withdraw(&id);
        }
    }
    for (base, item) in wanted {
        raise_once(notes, &base, item);
    }
}
