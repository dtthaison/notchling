use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, button_with, label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::triage::{Section, Triage};
use super::view::InboxPanelView;

const TAB_PADDING: f32 = 6.0;
const TAB_GAP: f32 = 2.0;
const TAB_COUNT_GAP: f32 = 3.0;

pub(super) fn tab_name(section: Section) -> &'static str {
    match section {
        Section::Waiting => t::inbox::tab_waiting(),
        Section::Running => t::inbox::tab_running(),
        Section::DidntWork => t::inbox::tab_didnt_work(),
        Section::Info => t::inbox::tab_info(),
        Section::Done => t::inbox::tab_done(),
        Section::Blocking => t::inbox::blocking_title(),
    }
}

pub(super) fn tab_empty(section: Section) -> &'static str {
    match section {
        Section::Waiting | Section::Blocking => t::inbox::empty_waiting(),
        Section::Running => t::inbox::empty_running(),
        Section::DidntWork => t::inbox::empty_didnt_work(),
        Section::Info => t::inbox::empty_info(),
        Section::Done => t::inbox::empty_done(),
    }
}

impl InboxPanelView {
    pub(super) fn tab_bar(
        &self,
        triage: &Triage,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut bar = div()
            .id("inbox-tabs")
            .px(px(SPACE[1]))
            .pt(px(SPACE[1]))
            .pb(px(SPACE[0]))
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap(px(TAB_GAP))
            .overflow_x_scroll()
            .track_scroll(&self.tabs_scroll);
        for section in Section::TABS {
            let count = triage.rows(section).len();
            let kind = if section == self.tab {
                ButtonKind::Secondary
            } else {
                ButtonKind::Ghost
            };
            let content = div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap(px(TAB_COUNT_GAP))
                .whitespace_nowrap()
                .child(tab_name(section))
                .child(label(&count.to_string(), TypeRole::Meta, Ink::TextMuted, a));
            let id = SharedString::from(format!("inbox-tab-{section:?}"));
            bar = bar.child(
                button_with(id, content, kind, PartState::Rest, a)
                    .px(px(TAB_PADDING))
                    .flex_shrink_0()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_tab(section);
                        cx.notify();
                    })),
            );
        }
        bar
    }

    pub(super) fn blocking_heading(&self, count: usize, a: Appearance) -> Div {
        div()
            .px(px(SPACE[2]))
            .pt(px(SPACE[1]))
            .pb(px(SPACE[0]))
            .flex()
            .flex_wrap()
            .items_baseline()
            .gap_x(px(SPACE[1]))
            .child(label(
                t::inbox::blocking_title(),
                TypeRole::Label,
                Ink::Needs,
                a,
            ))
            .child(label(
                &t::inbox::blocking_count(count as i64),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
            .child(label(
                t::inbox::blocking_hint(),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
    }

    pub(super) fn calm_block(&self, triage: &Triage, a: Appearance) -> Div {
        div()
            .px(px(SPACE[2]))
            .py(px(SPACE[1]))
            .flex()
            .flex_col()
            .gap(px(SPACE[0]))
            .child(label(
                t::inbox::calm_title(),
                TypeRole::Heading,
                Ink::Text,
                a,
            ))
            .child(label(
                t::inbox::calm_body(),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
            .children(
                triage
                    .last_answer
                    .as_ref()
                    .map(|line| label(line, TypeRole::Meta, Ink::TextFaint, a)),
            )
    }

    pub(super) fn empty_line(&self, text: &str, a: Appearance) -> Div {
        div().px(px(SPACE[2])).py(px(SPACE[1])).child(label(
            text,
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ))
    }

    pub(super) fn keys_hint(&self, a: Appearance) -> Div {
        div()
            .px(px(SPACE[2]))
            .py(px(SPACE[1]))
            .child(label(t::inbox::keys_hint(), TypeRole::Meta, Ink::TextFaint, a).elide())
    }

    pub(super) fn clear_button(
        &self,
        id: &str,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let row = id.to_owned();
        button(
            SharedString::from(format!("inbox-clear-{id}")),
            t::inbox::clear(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.feed.clear_row(&row);
            cx.notify();
        }))
    }
}
