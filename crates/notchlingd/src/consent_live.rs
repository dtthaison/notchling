use crate::consent_ops::{lock, ConsentOps};
use crate::presence::PresenceGuard;
use notchling_engine::inbox::{Live, Waiter};

impl ConsentOps {
    pub(crate) fn here(&self, id: &str) -> PresenceGuard {
        let guard = self.presence.enter(id, self.now());
        self.sweep_live();
        guard
    }

    pub(crate) fn sweep_live(&self) {
        for (id, here) in self.presence.sweep(self.now()) {
            if !here {
                lock(&self.restored).remove(&id);
            }
            self.show(&id, here);
        }
    }

    pub(crate) fn show_live(&self, id: &str) {
        if self.presence.present(id, self.now()) {
            self.show(id, true);
        }
    }

    fn show(&self, id: &str, here: bool) {
        let Some(inbox) = &self.inbox else {
            return;
        };
        let Some(item) = lock(&self.items).get(id).cloned() else {
            return;
        };
        let live = here.then(|| Live {
            waiting_since: self.waiting_since(id),
            waiter: Waiter::Cli,
        });
        inbox.set_live(&item, live);
    }

    fn waiting_since(&self, id: &str) -> i64 {
        let restored = lock(&self.restored).contains(id);
        let created = restored
            .then(|| self.store.get(id).ok().flatten())
            .flatten()
            .map(|row| row.created_ms);
        created
            .or_else(|| self.presence.since(id))
            .unwrap_or_else(|| self.now())
    }
}
