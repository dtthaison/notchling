use crate::cluster_relay::HeldRelay;
use crate::daemon_wiring::Wired;
use std::sync::atomic::Ordering;
use std::time::Duration;

const RELEASE_WAIT: Duration = Duration::from_secs(3);

impl Wired {
    pub fn release(&self) {
        let slot = self.cluster.held();
        self.release_within(slot.get().map(|relay| &**relay), RELEASE_WAIT);
    }

    pub(crate) fn release_within(&self, relay: Option<&HeldRelay>, wait: Duration) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(relay) = relay {
            relay.stop(wait);
        }
        self.bridge.release();
        self.outbox.release();
    }
}

impl Drop for Wired {
    fn drop(&mut self) {
        self.release();
    }
}
