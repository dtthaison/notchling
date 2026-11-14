use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button_with, hsla, icon, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::ChatView;
use crate::chat::pending_child::Step;
use crate::chat::subagent_view::header_text;

pub fn header(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let open = view.open_subagent.as_ref()?;
    let (title, detail) = header_text(&open.agent, open.conversation.as_deref());
    let back = button_with(
        "subagent-back",
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(icon(Icon::ChevronLeft, 10.0, Ink::Text, a))
            .child(label("Back", TypeRole::Label, Ink::Text, a)),
        ButtonKind::Ghost,
        PartState::Rest,
        a,
    )
    .flex_shrink_0()
    .on_click(cx.listener(|this, _, _, cx| {
        this.close_subagent(cx);
    }));
    Some(
        div()
            .w_full()
            .h(px(36.0))
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .flex()
            .items_center()
            .gap(px(10.0))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(back)
            .child(
                label(&title, TypeRole::Label, Ink::Text, a)
                    .flex_shrink()
                    .min_w(px(0.0))
                    .elide(),
            )
            .child(label(&detail, TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0())
            .part_at("Subagent Header", Corner::Trailing, a, cx),
    )
}

impl ChatView {
    pub(super) fn open_subagent(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(agent) = crate::chat::subagent_view::find(&self.subagents, id) else {
            return;
        };
        self.open_subagent = Some(crate::chat::subagent_view::Open::new(agent));
        self.scroll_to_end_now(cx);
        self.reload_now(cx);
        cx.notify();
    }

    pub(super) fn load_more_subagents(&mut self, cx: &mut Context<Self>) {
        self.more_subagents += 1;
        self.reload_now(cx);
    }

    pub(super) fn open_pending_child(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_child.as_mut() else {
            return;
        };
        match pending.after_load(&self.subagents) {
            Step::Open(id) => {
                self.pending_child = None;
                self.open_subagent(&id, cx);
            }
            Step::Waiting => {}
            Step::GaveUp => self.pending_child = None,
        }
    }

    pub(super) fn toggle_group(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.open_groups.remove(id) {
            self.open_groups.insert(id.to_owned());
        }
        self.reload_now(cx);
    }

    pub(super) fn load_more_under(&mut self, id: &str, cx: &mut Context<Self>) {
        *self.more_under.entry(id.to_owned()).or_default() += 1;
        self.reload_now(cx);
    }

    pub(super) fn close_subagent(&mut self, cx: &mut Context<Self>) {
        if self.open_subagent.take().is_some() {
            self.scroll_to_end_now(cx);
            cx.notify();
        }
    }
}
