use crate::daemon_wiring::{Ops, Wired};
use crate::layout::Layout;
use crate::update_asks::UpdateAsks;

pub fn wire(ops: Ops, wired: &Wired, _layout: &Layout) -> Ops {
    let install = wired.switches.install.clone();
    let asks = UpdateAsks::new(
        wired.consent.clone(),
        wired.versions.clone(),
        std::sync::Arc::new(move || install.is_on()),
    );
    ops.with_group(asks.clone()).with_cited(asks)
}
