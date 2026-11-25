use crate::daemon_wiring::{Ops, Wired};
use crate::layout::Layout;

#[path = "machine_asks.rs"]
pub mod machine_asks;
#[path = "machine_asks_live.rs"]
pub mod machine_asks_live;
#[path = "machine_asks_plans.rs"]
pub mod machine_asks_plans;
#[path = "machines_ops.rs"]
pub mod machines_ops;

pub fn wire(ops: Ops, wired: &Wired, layout: &Layout) -> Ops {
    let machines = machine_asks_live::build(wired, layout);
    let asks = machine_asks::MachineAsks::new(machines.clone());
    ops.with_group(std::sync::Arc::new(machines_ops::Reads(machines)))
        .with_group(asks.clone())
        .with_cited(asks)
}

#[cfg(test)]
#[path = "machine_asks_live_tests.rs"]
mod machine_asks_live_tests;
#[cfg(test)]
#[path = "machine_asks_rig.rs"]
pub mod machine_asks_rig;
#[cfg(test)]
#[path = "machine_asks_tests.rs"]
mod machine_asks_tests;
#[cfg(test)]
#[path = "machines_ops_tests.rs"]
mod machines_ops_tests;
