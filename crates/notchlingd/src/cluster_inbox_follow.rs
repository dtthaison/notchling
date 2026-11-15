use crate::inbox_mirrors::{localize, InboxLink};
use crate::inbox_ops::InboxOps;
use notchling_engine::inbox::{Change, Item};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub const POLL_MS: i64 = 2_000;

#[derive(Default)]
struct Seen {
    shown: BTreeMap<String, Item>,
    next_at: i64,
}

pub struct Follow {
    inbox: Arc<InboxOps>,
    link: Arc<dyn InboxLink>,
    seen: Mutex<Seen>,
}

impl Follow {
    pub fn new(inbox: Arc<InboxOps>, link: Arc<dyn InboxLink>) -> Arc<Follow> {
        Arc::new(Follow {
            inbox,
            link,
            seen: Mutex::new(Seen::default()),
        })
    }

    pub fn poll(&self, now: i64) {
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if now < seen.next_at {
                return;
            }
            seen.next_at = now + POLL_MS;
        }
        let from_home = self
            .link
            .me()
            .zip(self.link.home_items())
            .map(|(me, (home, items))| localize(&me, &home, items));
        let known = self.inbox.local_ids();
        let current: BTreeMap<String, Item> = from_home
            .into_iter()
            .flatten()
            .filter(|item| !known.contains(&item.id))
            .map(|item| (item.id.clone(), item))
            .collect();
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        let mut changes = Vec::new();
        for (id, item) in &current {
            match seen.shown.get(id) {
                None => changes.push(Change::Raised(item.clone())),
                Some(old) if old != item => changes.push(Change::Updated(item.clone())),
                Some(_) => {}
            }
        }
        for id in seen.shown.keys().filter(|id| !current.contains_key(*id)) {
            changes.push(Change::Removed(id.clone()));
        }
        seen.shown = current;
        drop(seen);
        self.inbox.show_remote(changes);
    }
}
