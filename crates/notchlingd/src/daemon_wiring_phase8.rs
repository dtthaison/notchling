use crate::daemon_wiring::{Ops, Wired};
use crate::layout::Layout;

pub fn wire(ops: Ops, wired: &Wired, layout: &Layout) -> Ops {
    let ops = crate::daemon_wiring_embed::wire(ops, wired, layout);
    let ops = crate::daemon_wiring_machines::wire(ops, wired, layout);
    let ops = crate::daemon_wiring_install::wire(ops, wired, layout);
    crate::daemon_wiring_updates::wire(ops, wired, layout)
}
