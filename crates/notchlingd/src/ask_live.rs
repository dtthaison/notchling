use crate::ask_ops::AskOps;
use crate::presence::PresenceGuard;
use notchling_engine::inbox::{Live, Waiter};

impl AskOps {
    pub(crate) fn here(&self, id: &str) -> PresenceGuard {
        let guard = self.presence.enter(id, self.now());
        self.sweep_live(self.now());
        guard
    }

    pub(crate) fn sweep_live(&self, now_ms: i64) {
        for (id, here) in self.presence.sweep(now_ms) {
            self.show(&id, here);
        }
    }

    pub(crate) fn show_live(&self, id: &str) {
        if self.presence.present(id, self.now()) {
            self.show(id, true);
        }
    }

    fn show(&self, id: &str, here: bool) {
        let Some(link) = self.link.get() else {
            return;
        };
        let Ok(Some(row)) = self.store.get(id) else {
            return;
        };
        let Some(inbox_id) = row.inbox_id else {
            return;
        };
        let live = here.then(|| Live {
            waiting_since: self.presence.since(id).unwrap_or_else(|| self.now()),
            waiter: Waiter::Ask,
        });
        link.inbox.set_live(&inbox_id, live);
    }
}
