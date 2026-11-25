use crate::acting_ops::ActingOps;
use crate::agy_ops::AgyOps;
use crate::codex_ops::CodexOps;
use crate::layout::Layout;
use crate::outbox_courier::{real_clock, ClaudeCourier};
use crate::outbox_ops::OutboxOps;
use crate::outbox_postman::{Couriers, Postmen};
use crate::outbox_runners::{link_agy, link_codex, AgyCourier, CodexCourier};
use crate::outbox_verify::{SafeModes, TranscriptVerifier};
use std::sync::Arc;

pub(crate) fn postmen_for(
    outbox: &Arc<OutboxOps>,
    acting: &Arc<ActingOps>,
    layout: &Layout,
    codex: &Arc<CodexOps>,
    agy: &Arc<AgyOps>,
) -> Postmen {
    link_codex(outbox, codex);
    link_agy(outbox, agy);
    let safe = Arc::new(SafeModes::default());
    let courier = Arc::new(ClaudeCourier::new(
        acting.clone(),
        layout.roots(),
        real_clock(),
        safe.clone(),
    ));
    let resume = courier.clone();
    outbox.set_resume(Arc::new(move |row| resume.resume(row)));
    let postmen = Postmen::build(
        outbox.clone(),
        Couriers {
            claude: courier,
            codex: Arc::new(CodexCourier::new(codex.clone())),
            antigravity: Arc::new(AgyCourier::new(agy.clone())),
        },
        Arc::new(TranscriptVerifier::new(layout.roots(), safe)),
        acting.acting_switch(),
        real_clock(),
    );
    outbox.set_wake(postmen.waker());
    postmen
}
