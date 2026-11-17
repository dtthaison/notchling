use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::add_sheet::{brief, not_joined_line};
use super::model::ClusterModel;
use super::not_joined::NotJoined;
use super::view::ClusterSheetView;

impl ClusterSheetView {
    pub(super) fn add_rows(&self, a: Appearance, cx: &mut Context<Self>) -> Vec<Div> {
        let runs = self.feed.adds();
        runs.briefs()
            .into_iter()
            .filter_map(|(name, phase)| brief(&name, &phase))
            .map(|row| {
                let mut line = div().flex().items_center().gap(px(SPACE[1])).child(
                    div().flex_1().min_w(px(0.0)).child(label(
                        &row.text,
                        TypeRole::Meta,
                        if row.problem {
                            Ink::Needs
                        } else {
                            Ink::TextMuted
                        },
                        a,
                    )),
                );
                let show = row.name.clone();
                line = line.child(
                    button(
                        SharedString::from(format!("add-row-show-{}", row.name)),
                        t::cluster::add_row_show(),
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.show_add(&show, cx))),
                );
                if row.dismiss {
                    let name = row.name.clone();
                    line = line.child(
                        button(
                            SharedString::from(format!("add-row-dismiss-{}", row.name)),
                            t::cluster::add_row_dismiss(),
                            ButtonKind::Ghost,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.feed.adds().dismiss(&name);
                            cx.notify();
                        })),
                    );
                }
                line
            })
            .collect()
    }

    pub(super) fn not_joined_block(
        &mut self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let list = self.not_joined_list(model);
        if list.is_empty() {
            return None;
        }
        let mut block = div().flex().flex_col().gap(px(SPACE[1])).child(label(
            t::cluster::not_joined_title(),
            TypeRole::Label,
            Ink::TextFaint,
            a,
        ));
        for machine in &list {
            block = block.child(self.not_joined_row(machine, a, cx));
        }
        Some(block.part("Not in the Cluster", a, cx))
    }

    fn not_joined_row(&self, machine: &NotJoined, a: Appearance, cx: &mut Context<Self>) -> Div {
        let name = machine.name.clone();
        let join = button(
            SharedString::from(format!("not-joined-join-{}", machine.name)),
            t::cluster::not_joined_join(),
            ButtonKind::Secondary,
            if self.busy {
                PartState::Disabled
            } else {
                PartState::Rest
            },
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.open_join(&name, cx)));
        card(a).p(px(SPACE[2])).child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[0] / 2.0))
                        .child(label(&machine.name, TypeRole::Heading, Ink::Text, a).elide())
                        .child(label(
                            &not_joined_line(machine),
                            TypeRole::Meta,
                            Ink::TextMuted,
                            a,
                        )),
                )
                .child(join),
        )
    }
}
