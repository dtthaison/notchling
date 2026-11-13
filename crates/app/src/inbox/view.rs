use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyView, Context, Div, Entity, Render, SharedString, Stateful, Window,
};
use notchling_engine::inbox::ActionId;
use notchling_kit::metric::{SIDE_PADDING, SPACE};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon, label, scrollable, window_appearance, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};
use serde_json::Value;

use super::feed::InboxFeed;
use super::model::{is_local, target, Row, Target};
use super::renders::{model, shown, NOW_MS};
use super::{MAX_HEIGHT, WIDTH};
use crate::phone::bridge::now_ms;

const POLL: Duration = Duration::from_millis(500);
const CLOCK_EVERY: u32 = 20;

pub struct InboxPanelView {
    feed: Arc<InboxFeed>,
    fixed_now: Option<i64>,
    done_open: bool,
    sent_seen: HashSet<String>,
    busy: HashSet<String>,
    note: Option<String>,
    drawn: u64,
}

impl InboxPanelView {
    pub fn live(feed: Arc<InboxFeed>, cx: &mut Context<Self>) -> InboxPanelView {
        let mut view = InboxPanelView {
            feed,
            fixed_now: None,
            done_open: false,
            sent_seen: HashSet::new(),
            busy: HashSet::new(),
            note: None,
            drawn: 0,
        };
        view.send_seen(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(POLL).await;
                ticks = ticks.wrapping_add(1);
                if this.update(cx, |view, cx| view.poll(ticks, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        view
    }

    pub fn fixed(feed: Arc<InboxFeed>, now_ms: i64, done_open: bool) -> InboxPanelView {
        InboxPanelView {
            feed,
            fixed_now: Some(now_ms),
            done_open,
            sent_seen: HashSet::new(),
            busy: HashSet::new(),
            note: None,
            drawn: 0,
        }
    }

    fn poll(&mut self, ticks: u32, cx: &mut Context<Self>) {
        let changed = self.feed.generation() != self.drawn;
        if changed || ticks.is_multiple_of(CLOCK_EVERY) {
            cx.notify();
        }
        if changed {
            self.note = None;
            self.send_seen(cx);
        }
    }

    fn send_seen(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .feed
            .snapshot(0)
            .unseen_ids()
            .into_iter()
            .filter(|id| !self.sent_seen.contains(id))
            .collect();
        if ids.is_empty() || !self.feed.is_live() {
            return;
        }
        self.sent_seen.extend(ids.iter().cloned());
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let sent_ids = ids.clone();
            let sent = cx
                .background_executor()
                .spawn(async move { feed.seen(&sent_ids) })
                .await;
            if let Err(e) = sent {
                eprintln!("inbox: marking rows seen wasn't sent ({e})");
                this.update(cx, |view, _| {
                    for id in &ids {
                        view.sent_seen.remove(id);
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    fn bring_up(&mut self, open: Option<&Value>) {
        match open.and_then(target) {
            Some(Target::Approval(ticket)) | Some(Target::Request(ticket)) => {
                crate::mac::vault_prompts::raise(&ticket);
            }
            None => {}
        }
    }

    fn act(&mut self, id: String, action: ActionId, open: Option<Value>, cx: &mut Context<Self>) {
        if is_local(action) {
            self.bring_up(open.as_ref());
            return;
        }
        if !self.busy.insert(id.clone()) {
            return;
        }
        self.note = None;
        let feed = self.feed.clone();
        let sent = id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { feed.answer(&sent, action) })
                .await;
            this.update(cx, |view, cx| {
                view.busy.remove(&id);
                if let Err(e) = result {
                    view.note = Some(e);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle_done(&mut self, cx: &mut Context<Self>) {
        self.done_open = !self.done_open;
        cx.notify();
    }

    fn action_buttons(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        let live = row.enabled && !self.busy.contains(&row.id);
        let mut buttons = div().flex().flex_wrap().gap(px(SPACE[1]));
        for (action, text, primary) in &row.actions {
            let kind = if *primary && row.enabled {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            let state = if live {
                PartState::Rest
            } else {
                PartState::Disabled
            };
            let id = SharedString::from(format!("inbox-{}-{action:?}", row.id));
            let mut one = button(id, text, kind, state, a);
            if live {
                let (row_id, action, open) = (row.id.clone(), *action, row.open.clone());
                one = one.on_click(cx.listener(move |this, _, _, cx| {
                    this.act(row_id.clone(), action, open.clone(), cx);
                }));
            }
            buttons = buttons.child(one);
        }
        buttons
    }

    fn body(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let ink = if row.enabled {
            Ink::Text
        } else {
            Ink::TextMuted
        };
        let top = div()
            .flex()
            .items_start()
            .gap(px(SPACE[1]))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(label(&row.title, TypeRole::Heading, ink, a).elide()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .child(label(&row.ago, TypeRole::Meta, Ink::TextFaint, a)),
            );
        let mut body = div()
            .id(SharedString::from(format!("inbox-body-{}", row.id)))
            .flex()
            .flex_col()
            .gap(px(SPACE[0] / 2.0))
            .child(top)
            .children((!row.detail.is_empty()).then(|| {
                label(&row.detail, TypeRole::Meta, Ink::TextMuted, a)
                    .w_full()
                    .elide()
            }));
        if row.enabled && row.open.as_ref().and_then(target).is_some() {
            let open = row.open.clone();
            body = body
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.bring_up(open.as_ref());
                    cx.notify();
                }));
        }
        body
    }

    fn row(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .px(px(SPACE[2]))
            .py(px(SPACE[1] + SPACE[0] / 2.0))
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(self.body(row, a, cx))
            .children((!row.actions.is_empty()).then(|| self.action_buttons(row, a, cx)))
            .part("Inbox Row", a, cx)
    }

    fn heading(&self, text: &str, a: Appearance) -> Div {
        div()
            .px(px(SPACE[2]))
            .pt(px(SPACE[1]))
            .pb(px(SPACE[0]))
            .child(label(text, TypeRole::Label, Ink::TextFaint, a))
    }

    fn done_heading(&self, count: usize, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let chevron = if self.done_open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        div()
            .id("inbox-done")
            .cursor_pointer()
            .px(px(SPACE[2]))
            .pt(px(SPACE[1]))
            .pb(px(SPACE[0]))
            .flex()
            .items_center()
            .gap(px(SPACE[0]))
            .child(icon(chevron, IconSize::Row.points(), Ink::TextFaint, a))
            .child(label("Done", TypeRole::Label, Ink::TextFaint, a))
            .child(label(&count.to_string(), TypeRole::Meta, Ink::TextFaint, a))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_done(cx)))
    }
}

impl Render for InboxPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        self.drawn = self.feed.generation();
        let now = self.fixed_now.unwrap_or_else(now_ms);
        let (needs, done) = self.feed.snapshot(now).sections();
        let mut list = div()
            .id("inbox-list")
            .min_h(px(0.0))
            .pb(px(SPACE[1]))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if needs.is_empty() {
            list = list.child(div().px(px(SPACE[2])).py(px(SPACE[1])).child(label(
                "Nothing needs you",
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )));
        } else {
            list = list.child(self.heading("Needs You", a));
            for row in &needs {
                list = list.child(self.row(row, a, cx));
            }
        }
        if !done.is_empty() {
            list = list.child(self.done_heading(done.len(), a, cx));
            if self.done_open {
                for row in &done {
                    list = list.child(self.row(row, a, cx));
                }
            }
        }
        let header = div()
            .px(px(SPACE[2]))
            .pt(px(SPACE[2]))
            .pb(px(SPACE[0]))
            .child(label("Inbox", TypeRole::Title, Ink::Text, a));
        let note = self.note.as_ref().map(|n| {
            div()
                .px(px(SPACE[2]))
                .child(label(n, TypeRole::Meta, Ink::Needs, a))
        });
        card(a)
            .w(px(WIDTH))
            .max_h(px(MAX_HEIGHT))
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .shadow_lg()
            .child(header)
            .children(note)
            .child(scrollable(list))
            .part("Inbox Panel", a, cx)
    }
}

struct InboxRender {
    panel: Entity<InboxPanelView>,
}

impl Render for InboxRender {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        div()
            .size_full()
            .p(px(SIDE_PADDING))
            .fill(Ink::Window, a)
            .child(self.panel.clone())
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let done_open = shown(name)?;
    let feed = Arc::new(InboxFeed::fixed(model()));
    let panel = cx.new(|_| InboxPanelView::fixed(feed, NOW_MS, done_open));
    Some(cx.new(|_| InboxRender { panel }).into())
}
