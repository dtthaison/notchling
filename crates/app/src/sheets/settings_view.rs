//! The Settings sheet and the banners over GPUI (parts U8 and U7, macOS). Settings writes Notchling's
//! own `settings.json` (never Claude Code's); acting shows read-only. A render has no store: its
//! rows show the defaults and a click changes only what is drawn.

use std::path::Path;

use gpui::{
    div, prelude::*, px, Context, Div, EventEmitter, Render, SharedString, Stateful, Window,
};
use notchling_engine::settings::Settings;
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use serde_json::{Map, Value};

use super::banner::Banners;
use super::settings::{change, sections, Field, Row};
use super::view::{sheet_header, Acts, Dismiss};

/// The Settings sheet.
pub struct SettingsView {
    store: Option<Settings>,
    /// What's drawn: the store's values, or a render's own.
    values: Map<String, Value>,
    acts: Acts,
    /// A write that failed, shown at the bottom.
    error: Option<String>,
}

impl EventEmitter<Dismiss> for SettingsView {}

impl SettingsView {
    /// Over the store in `data_dir`, or none (a render, a fixture root).
    pub fn new(data_dir: Option<&Path>, acts: Acts) -> SettingsView {
        let store = data_dir.and_then(|d| Settings::open(&d.join("settings.json")).ok());
        let values = store
            .as_ref()
            .map(|s| s.values().clone())
            .unwrap_or_default();
        SettingsView {
            store,
            values,
            acts,
            error: None,
        }
    }

    fn apply(&mut self, row: &Row, option: &str, cx: &mut Context<Self>) {
        let Some((key, value)) = change(row, option) else {
            return;
        };
        if let Some(store) = &mut self.store {
            if let Err(e) = store.set(key, value.clone()) {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        }
        self.error = None;
        self.values.insert(key.to_owned(), value);
        cx.notify();
    }

    fn field(&self, row: &Row, i: usize, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = |part: &str| SharedString::from(format!("setting-{i}-{part}"));
        match &row.field {
            Field::Toggle { on, .. } => {
                let r = row.clone();
                div().child(
                    switch(id("switch"), *on, a)
                        .on_click(cx.listener(move |this, _, _, cx| this.apply(&r, "", cx))),
                )
            }
            Field::Choice { options, selected } => div()
                .flex()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded(px(RADIUS_SM))
                .fill(Ink::Raised, a)
                .children(options.iter().map(|(value, title)| {
                    let r = row.clone();
                    let value = (*value).to_owned();
                    let kind = if *selected == value {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    };
                    button(id(&value), title, kind, PartState::Rest, a)
                        .on_click(cx.listener(move |this, _, _, cx| this.apply(&r, &value, cx)))
                })),
            Field::Stepper { value, .. } => {
                let (down, up) = (row.clone(), row.clone());
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        button(id("down"), "−", ButtonKind::Secondary, PartState::Rest, a)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.apply(&down, "-1", cx)),
                            ),
                    )
                    .child(label(&format!("{value}%"), TypeRole::Label, Ink::Text, a))
                    .child(
                        button(id("up"), "+", ButtonKind::Secondary, PartState::Rest, a)
                            .on_click(cx.listener(move |this, _, _, cx| this.apply(&up, "+1", cx))),
                    )
            }
            Field::ReadOnly { value, .. } => div()
                .px(px(8.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .rounded_full()
                .fill(Ink::Raised, a)
                .child(label(value, TypeRole::Label, Ink::TextMuted, a)),
        }
    }
}

/// A switch: a capsule with its knob at the end it's set to.
fn switch(id: SharedString, on: bool, a: Appearance) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(32.0))
        .h(px(18.0))
        .p(px(2.0))
        .rounded_full()
        .flex()
        .when(on, |s| s.justify_end())
        .fill(if on { Ink::Accent } else { Ink::Raised }, a)
        .cursor_pointer()
        .child(div().size(px(14.0)).rounded_full().bg(gpui::white()))
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let values = self.values.clone();
        let read = move |key: &str| values.get(key).cloned();
        let acting = self.acts.acting();
        let done = button(
            "settings-done",
            "Done",
            ButtonKind::Primary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss)));
        let mut list = div()
            .id("settings-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(14.0));
        let mut n = 0;
        for section in sections(&read, acting) {
            let mut rows = card(a).flex().flex_col();
            for (k, row) in section.rows.iter().enumerate() {
                n += 1;
                let note = match &row.field {
                    Field::ReadOnly {
                        note: Some(note), ..
                    } => Some(label(note, TypeRole::Meta, Ink::TextFaint, a)),
                    _ => None,
                };
                rows = rows.child(
                    div()
                        .px(px(14.0))
                        .py(px(8.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .when(k > 0, |r| {
                            r.border_t(px(1.0))
                                .border_color(hsla(Ink::Hairline.color(a)))
                        })
                        .child(
                            div()
                                .min_h(px(24.0))
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .child(label(row.title, TypeRole::Body, Ink::Text, a).flex_1())
                                .child(self.field(row, n, a, cx)),
                        )
                        .children(note),
                );
            }
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(label(section.title, TypeRole::Section, Ink::TextFaint, a))
                    .child(rows)
                    .children(
                        section
                            .footer
                            .map(|f| label(f, TypeRole::Meta, Ink::TextFaint, a)),
                    ),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header(Icon::More, "Settings", done, a))
            .child(list)
            .children(self.error.as_deref().map(|e| {
                label(e, TypeRole::Meta, Ink::Needs, a)
                    .px(px(SIDE_PADDING))
                    .pb(px(10.0))
            }))
    }
}

/// The banners on show, each with Dismiss.
pub struct BannersView {
    pub banners: Banners,
}

impl Render for BannersView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let rows = self.banners.all().iter().enumerate().map(|(i, b)| {
            let dismiss = button(
                SharedString::from(format!("banner-dismiss-{i}")),
                "×",
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.banners.dismiss(i);
                cx.notify();
            }));
            let fill = match b.kind {
                super::banner::Kind::Error => Ink::Card,
                _ => Ink::AccentWash,
            };
            div()
                .w_full()
                .pl(px(14.0))
                .pr(px(6.0))
                .py(px(6.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(RADIUS_MD))
                .fill(fill, a)
                .border(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .child(label(b.icon().glyph(), TypeRole::Label, b.ink(), a))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .children(
                            b.title
                                .as_deref()
                                .map(|t| label(t, TypeRole::Heading, Ink::Text, a)),
                        )
                        .child(label(&b.text, TypeRole::Label, Ink::Text, a)),
                )
                .child(dismiss)
        });
        div()
            .size_full()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .fill(Ink::Window, a)
            .children(rows)
    }
}
