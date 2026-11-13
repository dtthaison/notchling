use std::collections::HashSet;
use std::sync::mpsc::{self, Sender};
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
        let notification = Notification::new(item.title.clone(), item.detail.clone());
        if let Err(e) = notifier.notify(&notification) {
            eprintln!("inbox: the system notification wasn't shown ({e})");
        }
    }
}

pub struct Detached(Option<Sender<Notification>>);

impl Detached {
    pub fn new(inner: Arc<dyn Notifier>) -> Detached {
        let (sender, receiver) = mpsc::channel::<Notification>();
        let spawned = std::thread::Builder::new()
            .name("notchling-inbox-banner".into())
            .spawn(move || {
                for notification in receiver {
                    if let Err(e) = inner.notify(&notification) {
                        eprintln!("inbox: the system notification wasn't shown ({e})");
                    }
                }
            });
        Detached(spawned.ok().map(|_| sender))
    }
}

impl Notifier for Detached {
    fn notify(&self, notification: &Notification) -> Result<()> {
        let Some(sender) = &self.0 else {
            return Err(Error::System("the banner thread did not start".into()));
        };
        sender
            .send(notification.clone())
            .map_err(|e| Error::System(e.to_string()))
    }
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
