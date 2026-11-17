use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_engine::machines::registry::Machine;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, label};
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

    fn join_cluster(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.model.join_cluster(name) {
            return;
        }
        if self.fixed {
            self.model.request_join_plan(name);
            return;
        }
        let engine = self.model.parts.engine.clone();
        let names = vec![name.to_owned()];
        let name = name.to_owned();
        Self::background(
            cx,
            move || engine.refresh(Some(&names), true),
            move |view, _, cx| {
                view.ask_cluster(cx);
                view.model.request_join_plan(&name);
            },
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
        if !on {
            let join = button(
                SharedString::from(format!("join-cluster-{}", machine.name)),
                "Join Cluster",
                ButtonKind::Secondary,
                if enabled {
                    PartState::Rest
                } else {
                    PartState::Disabled
                },
                a,
            );
            let name = machine.name.clone();
            row = row.child(join.when(enabled, |b| {
                b.on_click(cx.listener(move |this, _, window, cx| {
                    this.join_cluster(&name, cx);
                    window.focus(&this.focus);
                    cx.notify();
                }))
            }));
        }
        if let Some(line) = self.model.can_be_master_line(machine) {
            row = row.child(label(&line, TypeRole::Meta, Ink::TextMuted, a));
        }
        if let Some(line) = self.model.join_line(machine) {
            row = row.child(label(line, TypeRole::Meta, Ink::TextMuted, a));
        }
        row
    }
}
