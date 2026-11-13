use std::sync::Arc;

use gpui::{
    div, prelude::*, px, AnyView, Context, Div, Entity, Render, SharedString, Subscription, Window,
};
use notchling_engine::pages::clock::local_offset_secs;
use notchling_kit::metric::{RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon, label, scrollable, window_appearance, KitStyled, TextEditor,
    TextEditorEvent,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::model::{time_label, ActivityModel, Entry, Kind};
use super::renders::{model as fixture, shown, NOW_MS, UTC};
use super::wire;
use crate::acting::Acting;
use crate::actions::Actions;
use crate::phone::bridge::now_ms;
use crate::sheets::agents::Page;
use crate::sheets::settings_view::SettingsView;
use crate::sheets::view::Acts;

pub const SHOWN: usize = 300;

const BADGE_COLUMN: f32 = 112.0;

pub struct ActivityView {
    model: ActivityModel,
    actions: Option<Arc<Actions>>,
    search: Entity<TextEditor>,
    fixed: Option<(i64, i64)>,
    _search_events: Subscription,
}

impl ActivityView {
    pub fn live(actions: Arc<Actions>, cx: &mut Context<Self>) -> ActivityView {
        let mut view = ActivityView::build(ActivityModel::default(), Some(actions), None, cx);
        view.refresh(cx);
        view
    }

    pub fn fixed(model: ActivityModel, now_ms: i64, offset: i64, cx: &mut Context<Self>) -> Self {
        let search = model.search.clone();
        let view = ActivityView::build(model, None, Some((now_ms, offset)), cx);
        view.search
            .update(cx, |editor, cx| editor.set_text(&search, cx));
        view
    }

    fn build(
        model: ActivityModel,
        actions: Option<Arc<Actions>>,
        fixed: Option<(i64, i64)>,
        cx: &mut Context<Self>,
    ) -> ActivityView {
        let search =
            cx.new(|cx| TextEditor::new("Search Operation or Result", 1, cx).single_line());
        let events = cx.subscribe(&search, |this, editor, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Changed {
                this.model.search = editor.read(cx).text().to_owned();
                cx.notify();
            }
        });
        ActivityView {
            model,
            actions,
            search,
            fixed,
            _search_events: events,
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        if !self.model.start() {
            return;
        }
        let request = wire::request();
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { actions.read(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.model.finish(reply);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn clock(&self) -> (i64, i64) {
        self.fixed.unwrap_or_else(|| {
            let now = now_ms();
            (now, local_offset_secs(now as f64 / 1000.0))
        })
    }

    fn chips(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_wrap()
            .gap(px(6.0))
            .children(Kind::ALL.into_iter().map(|kind| {
                let on = self.model.is_on(kind);
                let count = self.model.count(kind);
                let text = if count > 0 {
                    format!("{} {count}", kind.label())
                } else {
                    kind.label().to_owned()
                };
                let (fill, ink) = if on {
                    (Ink::AccentWash, Ink::AccentText)
                } else {
                    (Ink::Raised, Ink::TextMuted)
                };
                div()
                    .id(SharedString::from(format!("activity-chip-{}", kind.wire())))
                    .h(px(24.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .fill(fill, a)
                    .cursor_pointer()
                    .child(label(&text, TypeRole::Label, ink, a))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.toggle(kind);
                        cx.notify();
                    }))
            }))
    }

    fn search_box(&self, a: Appearance) -> Div {
        div()
            .h(px(30.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(RADIUS_SM))
            .fill(Ink::Card, a)
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(icon(
                Icon::Search,
                TypeRole::Label.spec().size,
                Ink::TextFaint,
                a,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .kit_type(TypeRole::Body)
                    .ink(Ink::Text, a)
                    .child(self.search.clone()),
            )
    }

    fn entry(&self, e: &Entry, first: bool, clock: (i64, i64), a: Appearance) -> Div {
        let ink = match e.kind {
            Kind::Refused => Ink::Needs,
            Kind::Failed => Ink::Expired,
            _ => Ink::TextMuted,
        };
        let loud = e.kind.stands_out();
        let badge = div().flex_shrink_0().w(px(BADGE_COLUMN)).flex().child(
            div()
                .px(px(8.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .rounded_full()
                .fill(Ink::Raised, a)
                .child(label(e.kind.label(), TypeRole::Label, ink, a)),
        );
        div()
            .px(px(14.0))
            .py(px(8.0))
            .flex()
            .items_start()
            .gap(px(10.0))
            .when(!first, |r| {
                r.border_t(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
            })
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(3.0))
                    .h(px(34.0))
                    .rounded_full()
                    .when(loud, |bar| bar.fill(ink, a)),
            )
            .child(badge)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(label(&e.op, TypeRole::Body, Ink::Text, a).w_full().elide())
                    .child(
                        label(
                            &e.line(),
                            TypeRole::Meta,
                            if loud { ink } else { Ink::TextMuted },
                            a,
                        )
                        .w_full()
                        .elide(),
                    ),
            )
            .child(div().flex_shrink_0().child(label(
                &time_label(clock.0, e.time_ms, clock.1),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            )))
    }

    fn list(&self, a: Appearance) -> Div {
        let clock = self.clock();
        let visible = self.model.visible();
        let mut body = div().flex().flex_col().gap(px(8.0));
        if let Some(text) = self.model.empty_text() {
            let ink = match self.model.load {
                super::model::Load::Failed(_) => Ink::Needs,
                _ => Ink::TextMuted,
            };
            return body.child(div().py(px(24.0)).flex().justify_center().child(label(
                &text,
                TypeRole::Body,
                ink,
                a,
            )));
        }
        let shown = visible.len().min(SHOWN);
        body = body.child(
            card(a).flex().flex_col().children(
                visible
                    .iter()
                    .take(SHOWN)
                    .enumerate()
                    .map(|(i, e)| self.entry(e, i == 0, clock, a)),
            ),
        );
        let note = match (visible.len() > shown, self.model.truncated) {
            (true, _) => Some(format!(
                "Showing the newest {shown} of {} rows",
                visible.len()
            )),
            (false, true) => Some("The oldest rows are not shown".to_owned()),
            _ => None,
        };
        body.children(note.map(|n| label(&n, TypeRole::Meta, Ink::TextFaint, a)))
    }
}

impl Render for ActivityView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let loading = self.model.load == super::model::Load::Loading;
        let can_refresh = self.actions.is_some() && !loading;
        let refresh = {
            let state = if can_refresh {
                PartState::Rest
            } else {
                PartState::Disabled
            };
            let b = button(
                "activity-refresh",
                if loading {
                    "Refreshing\u{2026}"
                } else {
                    "Refresh"
                },
                ButtonKind::Secondary,
                state,
                a,
            );
            if can_refresh {
                b.on_click(cx.listener(|this, _, _, cx| this.refresh(cx)))
            } else {
                b
            }
        };
        let list = div()
            .id("activity-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .px(px(SIDE_PADDING))
            .pb(px(SIDE_PADDING))
            .child(self.list(a));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(SIDE_PADDING))
                    .pt(px(14.0))
                    .pb(px(10.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(self.search_box(a).flex_1())
                            .child(refresh),
                    )
                    .child(self.chips(a, cx)),
            )
            .child(scrollable(list))
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let which = shown(name)?;
    let acts = Acts {
        actions: Arc::new(Actions::fixed(Acting::Off)),
        sessions: None,
    };
    let activity = cx.new(|cx| ActivityView::fixed(fixture(which), NOW_MS, UTC, cx));
    Some(
        cx.new(|_| {
            SettingsView::new(None, acts)
                .with_page(Page::Activity)
                .with_activity(activity)
        })
        .into(),
    )
}
