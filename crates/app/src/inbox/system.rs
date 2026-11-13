use std::collections::HashSet;
use std::sync::Arc;

use notchling_engine::inbox::{ActionId, Item, State};
use notchling_platform::notify::{Notification, Notifier};
use notchling_platform::{Error, Result};

use super::model::InboxModel;

fn wants_you(item: &Item) -> bool {
    item.state == State::Open && item.actions.iter().any(|a| a.id != ActionId::Done)
}

pub fn remember(model: &InboxModel, notified: &mut HashSet<String>) {
    notified.extend(model.items().iter().map(|item| item.id.clone()));
}

pub fn notify_new(
    before: &InboxModel,
    after: &InboxModel,
    notified: &mut HashSet<String>,
    notifier: &dyn Notifier,
) {
    for item in after.items() {
        if before.items().iter().any(|held| held.id == item.id) {
            continue;
        }
        if !wants_you(item) || !notified.insert(item.id.clone()) {
            continue;
        }
        let _ = notifier.notify(&Notification::new(item.title.clone(), item.detail.clone()));
    }
}

pub struct Detached(Arc<dyn Notifier>);

impl Detached {
    pub fn new(inner: Arc<dyn Notifier>) -> Detached {
        Detached(inner)
    }
}

impl Notifier for Detached {
    fn notify(&self, notification: &Notification) -> Result<()> {
        let inner = self.0.clone();
        let notification = notification.clone();
        std::thread::Builder::new()
            .name("notchling-inbox-banner".into())
            .spawn(move || {
                let _ = inner.notify(&notification);
            })
            .map(|_| ())
            .map_err(|e| Error::System(e.to_string()))
    }
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
