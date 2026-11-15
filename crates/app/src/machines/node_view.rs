use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_engine::machines::registry::Machine;
use notchling_kit::ui::label;
use notchling_kit::{Appearance, Ink, TypeRole};

use super::jobs_view::switch;
use super::view::MachinesView;

impl MachinesView {
    pub(super) fn ask_cluster(&mut self, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let ask = self.model.cluster_ask();
        Self::background(
            cx,
            move || ask.run(),
            |view, answer, _| view.model.set_masters(answer),
        );
    }

    pub(super) fn node_row(
        &self,
        machine: &Machine,
        busy: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let on = self.model.node_on(machine);
        let enabled = !busy && self.model.can_add();
        let name = machine.name.clone();
        let control = switch(
            SharedString::from(format!("cluster-node-{}", machine.name)),
            on,
            enabled,
            a,
        )
        .when(enabled, |s| {
            s.on_click(cx.listener(move |this, _, _, cx| {
                let now = this.model.machine(&name).is_some_and(|m| m.node.is_some());
                if this.model.set_node(&name, !now) {
                    this.check(&name, cx);
                }
                cx.notify();
            }))
        });
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(label("Cluster Node", TypeRole::Label, Ink::Text, a))
            .child(control);
        if let Some(line) = self.model.can_be_master_line(machine) {
            row = row.child(label(&line, TypeRole::Meta, Ink::TextMuted, a));
        }
        row
    }
}
