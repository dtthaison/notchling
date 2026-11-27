use crate::cost_asks::CostAsks;
use crate::cost_profile_ops::CostProfileOps;
use crate::daemon_wiring::{Ops, Wired};
use crate::install_asks::InstallAsks;
use crate::layout::Layout;
use std::sync::Arc;

pub fn wire(ops: Ops, wired: &Wired, layout: &Layout) -> Ops {
    let install = InstallAsks::new(
        wired.consent.clone(),
        wired.install.clone(),
        wired.sidecar.clone(),
        wired.switches.install.clone(),
    );
    let planning = Arc::new(CostProfileOps::for_daemon(
        layout,
        wired.catalog.clone(),
        wired.switches.install.clone(),
    ));
    let cost = CostAsks::new(
        wired.consent.clone(),
        planning,
        wired.switches.install.clone(),
    );
    ops.with_group(install.clone())
        .with_cited(install)
        .with_group(cost.clone())
        .with_cited(cost)
}
