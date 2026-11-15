use super::{lock, InboxLink, InboxOps, Refusal, Tap};
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{Change, Item, State};
use std::collections::HashSet;
use std::sync::Arc;

impl InboxOps {
    pub fn local_ids(&self) -> HashSet<String> {
        self.local_view().into_iter().map(|item| item.id).collect()
    }

    pub fn own_open(&self) -> Vec<Item> {
        lock(&self.inner)
            .book
            .items()
            .into_iter()
            .filter(|item| item.state == State::Open)
            .collect()
    }

    pub fn set_link(&self, link: Arc<dyn InboxLink>) {
        *lock(&self.link) = Some(link);
    }

    pub fn set_tap(&self, tap: Tap) {
        lock(&self.inner).tap = Some(tap);
    }

    pub fn mirror_changed(&self, owner: &NodeId, item: Item) -> Result<(), Refusal> {
        let mut inner = lock(&self.inner);
        let changes = inner.mirrors.changed(owner, item)?;
        inner.show(changes);
        Ok(())
    }

    pub fn mirror_removed(&self, owner: &NodeId, id: &str) -> Result<(), Refusal> {
        let mut inner = lock(&self.inner);
        let change = inner.mirrors.removed(owner, id)?;
        inner.show(change.into_iter().collect());
        Ok(())
    }

    pub fn mirror_sync(&self, owner: &NodeId, open: Vec<Item>) -> Result<(), Refusal> {
        let mut inner = lock(&self.inner);
        let changes = inner.mirrors.sync(owner, open)?;
        inner.show(changes);
        Ok(())
    }

    pub fn keep_mirrors(&self, owners: &std::collections::BTreeSet<NodeId>) {
        let mut inner = lock(&self.inner);
        let changes = inner.mirrors.keep_owners(owners);
        inner.show(changes);
    }

    pub fn clear_mirrors(&self) {
        let mut inner = lock(&self.inner);
        let changes = inner.mirrors.clear();
        inner.show(changes);
    }

    pub fn show_remote(&self, changes: Vec<Change>) {
        lock(&self.inner).show(changes);
    }
}
