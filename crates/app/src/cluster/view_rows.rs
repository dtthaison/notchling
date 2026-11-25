use gpui::{deferred, div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, icon, icon_button, label, menu_panel, menu_rule, KitPart, KitStyled, MenuItem,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::model::{ClusterModel, NodeRow};
use super::secrets_view::{login_line, secrets_line};
use super::tool_updates::offers_runnable;
use super::tool_updates_view::ToolView;
use super::view::ClusterSheetView;
use crate::mac::gate;

impl ClusterSheetView {
    pub(super) fn dot(row: &NodeRow, a: Appearance) -> Stateful<Div> {
        let ink = if row.removed {
            Ink::TextFaint
        } else if row.pending {
            Ink::Needs
        } else if row.online {
            Ink::Done
        } else {
            Ink::Expired
        };
        let dot = div()
            .id(SharedString::from(format!("node-dot-{}", row.id)))
            .flex_shrink_0()
            .size(px(SPACE[1]))
            .rounded_full()
            .fill(ink, a);
        gate::tip_text(dot, row.state_word().into(), a)
    }

    pub(super) fn pill(text: &str, ink: Ink, wash: Ink, a: Appearance) -> Div {
        div()
            .h(px(notchling_kit::metric::PILL_HEIGHT))
            .px(px(SPACE[1]))
            .flex_shrink_0()
            .rounded_full()
            .fill(wash, a)
            .flex()
            .items_center()
            .child(label(text, TypeRole::Label, ink, a))
    }

    fn row_menu(
        &self,
        row: &NodeRow,
        tools: &ToolView,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = row.id.clone();
        let mut items = menu_panel(a);
        let master = id.clone();
        items = items.child(
            MenuItem::new(
                SharedString::from(format!("node-master-{id}")),
                t::cluster::can_be_master(),
            )
            .checked(row.can_be_master)
            .build(a)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pick(&master, ClusterModel::toggle_can_be_master, cx)
            })),
        );
        if row.offers_acting() {
            let acting = id.clone();
            items = items.child(
                MenuItem::new(
                    SharedString::from(format!("node-acting-{id}")),
                    if row.desired.0 {
                        t::cluster::acting_off()
                    } else {
                        t::cluster::acting_on()
                    },
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.pick(&acting, ClusterModel::toggle_acting, cx)
                })),
            );
        }
        if row.offers_acting() {
            let switch = id.clone();
            items = items.child(
                MenuItem::new(
                    SharedString::from(format!("node-install-switch-{id}")),
                    if row.desired.2 {
                        t::cluster::install_switch_off()
                    } else {
                        t::cluster::install_switch_on()
                    },
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.pick(&switch, ClusterModel::toggle_install, cx)
                })),
            );
        }
        if offers_runnable(&tools.offers, &id) {
            let update = id.clone();
            items = items.child(
                MenuItem::new(
                    SharedString::from(format!("node-update-tools-{id}")),
                    t::cluster::update_tools(),
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| this.start_tools(&update, None, cx))),
            );
        }
        if self.feed_offers_install(&id) {
            let hook = id.clone();
            items = items.child(
                MenuItem::new(
                    SharedString::from(format!("node-install-hook-{id}")),
                    t::cluster::install_hook(),
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| this.install_hook(&hook, cx))),
            );
        }
        if self.feed_offers_move(&id) {
            let to = id.clone();
            items = items.child(
                MenuItem::new(
                    SharedString::from(format!("node-move-{id}")),
                    t::cluster::move_to(&row.name),
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| this.open_move(&to, cx))),
            );
        }
        if self.feed_offers_take_back(&id) {
            items = items.child(
                MenuItem::new(
                    SharedString::from(format!("node-take-back-{id}")),
                    t::cluster::take_back(),
                )
                .build(a)
                .on_click(cx.listener(|this, _, _, cx| this.open_take_back(cx))),
            );
        }
        if row.offers_secrets() {
            let secrets = id.clone();
            items = items.child(
                MenuItem::new(
                    SharedString::from(format!("node-secrets-{id}")),
                    t::cluster::secrets(),
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| this.open_secrets(&secrets, cx))),
            );
        }
        if row.offers_remove() {
            let remove = id.clone();
            items = items.child(menu_rule(a)).child(
                MenuItem::new(
                    SharedString::from(format!("node-remove-{id}")),
                    t::cluster::remove(),
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.menu = None;
                    this.removing = Some(remove.clone());
                    cx.notify();
                })),
            );
        }
        div()
            .absolute()
            .right(px(SPACE[1]))
            .top(px(SPACE[4] + SPACE[1]))
            .child(
                div()
                    .id(SharedString::from(format!("cluster-click-away-{id}")))
                    .absolute()
                    .top(px(-4000.0))
                    .left(px(-4000.0))
                    .size(px(8000.0))
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu = None;
                        cx.notify();
                    })),
            )
            .child(items)
    }

    pub(super) fn node_row(
        &self,
        row: &NodeRow,
        fixes: &ToolView,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = row.id.clone();
        let mut title = div()
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .child(Self::dot(row, a))
            .child(label(&row.name, TypeRole::Heading, Ink::Text, a).elide())
            .child(Self::pill(row.role, Ink::AccentText, Ink::AccentWash, a));
        if row.me {
            title = title.child(Self::pill(
                t::cluster::this_mac(),
                Ink::TextMuted,
                Ink::Raised,
                a,
            ));
        }
        let mut head = div()
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .child(div().flex_1().min_w(px(0.0)).child(title));
        if self.feed_offers_take_back(&row.id) {
            head = head.child(
                button(
                    SharedString::from(format!("node-take-back-button-{id}")),
                    t::cluster::take_back(),
                    ButtonKind::Secondary,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_take_back(cx))),
            );
        }
        if row.offers_menu() {
            let open = self.menu.as_deref() == Some(row.id.as_str());
            let more = icon_button(
                SharedString::from(format!("node-more-{id}")),
                Icon::More,
                if open {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Ghost
                },
                PartState::Rest,
                a,
            );
            head = head.child(
                gate::tip_text(more, t::cluster::more().into(), a)
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(&id, cx))),
            );
        }
        let dim = if row.removed {
            Ink::TextFaint
        } else {
            Ink::TextMuted
        };
        let mut about: Vec<String> = Vec::new();
        about.extend((!row.os.is_empty()).then(|| row.os.clone()));
        about.extend(row.app_version.as_deref().map(t::cluster::app_version));
        let mut body = div().flex().flex_col().gap(px(SPACE[0] / 2.0)).child(head);
        if !about.is_empty() {
            body = body.child(label(&about.join(" · "), TypeRole::Meta, dim, a));
        }
        if let Some(load) = row.load_text() {
            body = body.child(label(&load, TypeRole::Meta, dim, a));
        }
        let tools = row.tool_texts();
        if !tools.is_empty() {
            body = body.child(label(&tools.join(" · "), TypeRole::Meta, Ink::TextFaint, a));
        }
        if let Some(replication) = &row.replication {
            if let Some(text) = replication.not_ready_text() {
                body = body.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(SPACE[0]))
                        .child(icon(
                            Icon::Warning,
                            TypeRole::Meta.spec().size,
                            Ink::Needs,
                            a,
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(label(
                            &text,
                            TypeRole::Meta,
                            Ink::Needs,
                            a,
                        ))),
                );
            } else {
                if let Some(text) = replication.set_text() {
                    body = body.child(label(&text, TypeRole::Meta, dim, a));
                }
                if let Some(text) = replication.behind_text() {
                    body = body.child(label(&text, TypeRole::Meta, Ink::Needs, a));
                }
            }
        }
        body = body
            .children(login_line(row, dim, a))
            .children(secrets_line(row, dim, a));
        for warning in &row.warnings {
            let (action, note) = self.tool_warning_extras(fixes, row, warning, a, cx);
            body = body.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[0]))
                    .child(icon(
                        Icon::Warning,
                        TypeRole::Meta.spec().size,
                        Ink::Needs,
                        a,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        warning,
                        TypeRole::Meta,
                        Ink::Needs,
                        a,
                    )))
                    .children(action),
            );
            body = body.children(note);
        }
        body = body.children(self.setup_lines(fixes, row, a, cx));
        body = body.children(self.tool_run_line(fixes, row, a, cx));
        let menu = (self.menu.as_deref() == Some(row.id.as_str()) && row.offers_menu())
            .then(|| deferred(self.row_menu(row, fixes, a, cx)).with_priority(1));
        card(a)
            .relative()
            .p(px(SPACE[2]))
            .child(body)
            .children(menu)
            .part_at("Node Row", Corner::TopTrailing, a, cx)
    }
}
