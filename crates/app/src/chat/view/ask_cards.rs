use std::cell::Cell;
use std::collections::HashMap;

use gpui::{
    div, prelude::*, px, Context, Div, FocusHandle, KeyDownEvent, SharedString, Stateful, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{CONTROL, RADIUS_COMPOSER, SIDE_PADDING};
use notchling_kit::part::ButtonKind;
use notchling_kit::ui::{button, field, hsla, icon, label, option_row, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::ChatView;
use crate::acting::Control;
use crate::chat::ask_drafts::{Act, Drafts, Keyed};
use crate::chat::asks::{card, AskRow, Card};
use crate::mac::gate;
use crate::phone::bridge::now_ms;

pub const ASK_ENV: &str = "NOTCHLING_RENDER_ASK";
const QUESTION_MAX_HEIGHT: f32 = 160.0;
const MINUTE_MS: i64 = 60_000;
const FOCUS_WAIT_MS: i64 = 5_000;

#[derive(Default)]
pub struct State {
    drafts: Drafts,
    focus: HashMap<String, FocusHandle>,
    preview: Option<Vec<AskRow>>,
    drawn_minute: Cell<Option<i64>>,
    wanted: Option<(String, i64)>,
}

impl State {
    pub(super) fn clock_due(&self, now_ms: i64) -> bool {
        self.drawn_minute
            .get()
            .is_some_and(|drawn| now_ms.div_euclid(MINUTE_MS) > drawn)
    }

    pub(super) fn not_drawn(&self) {
        self.drawn_minute.set(None);
    }

    pub fn preview(state: &str) -> State {
        State {
            preview: crate::chat::ask_preview::preview(state, now_ms()),
            ..State::default()
        }
    }
}

impl ChatView {
    fn ask_rows(&self) -> Vec<AskRow> {
        if let Some(preview) = &self.asks.preview {
            return preview.clone();
        }
        match self.selected_session() {
            Some(session) => self.source.ask_rows(&session.agent),
            None => Vec::new(),
        }
    }

    pub fn show_ask(&mut self, session: &str, id: &str, cx: &mut Context<Self>) {
        let row = crate::chat::pending_ask::row_for_key(&self.sessions, session);
        self.inbox_panel = None;
        if self.open_subagent.is_some() {
            self.close_subagent(cx);
        }
        match row {
            Some(row) => {
                self.pending_ask = None;
                self.select(&row, cx);
            }
            None => {
                self.pending_ask =
                    Some(crate::chat::pending_ask::PendingAsk::new(session, now_ms()));
                self.reload_now(cx);
            }
        }
        self.asks.wanted = Some((id.to_owned(), now_ms() + FOCUS_WAIT_MS));
        cx.notify();
    }

    pub(super) fn sync_asks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.ask_rows();
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let kept = match &self.asks.preview {
            Some(_) => Vec::new(),
            None => self.source.ask_ids(),
        };
        let kept: Vec<&str> = kept
            .iter()
            .map(String::as_str)
            .chain(ids.iter().copied())
            .collect();
        self.asks.drafts.keep_only(&kept);
        self.asks.focus.retain(|id, _| ids.contains(&id.as_str()));
        for row in rows.iter().filter(|r| r.is_open()) {
            if !self.asks.focus.contains_key(&row.id) {
                self.asks.focus.insert(row.id.clone(), cx.focus_handle());
            }
        }
        if let Some((id, until)) = self.asks.wanted.clone() {
            match self.asks.focus.get(&id) {
                Some(handle) => {
                    window.focus(handle);
                    self.asks.wanted = None;
                }
                None if now_ms() > until => self.asks.wanted = None,
                None => {}
            }
        }
    }

    pub(super) fn ask_clock_due(&self) -> bool {
        self.asks.clock_due(now_ms())
    }

    fn ask_gate(&self, cx: &mut Context<Self>) -> Control {
        if self.asks.preview.is_some() {
            return Control {
                enabled: true,
                tooltip: None,
            };
        }
        self.acting(cx).control()
    }

    fn ask_act(&mut self, id: &str, act: Act, cx: &mut Context<Self>) {
        if !self.ask_gate(cx).enabled {
            return;
        }
        let Some(row) = self.ask_rows().into_iter().find(|r| r.id == id) else {
            return;
        };
        let Some(request) = self.asks.drafts.act(&row, act) else {
            cx.notify();
            return;
        };
        self.send_ask(row.id, request, cx);
        cx.notify();
    }

    fn send_ask(&mut self, id: String, request: crate::acting::Request, cx: &mut Context<Self>) {
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.asks.drafts.settle(&id, result);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn ask_key(&mut self, id: &str, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(row) = self.ask_rows().into_iter().find(|r| r.id == id) else {
            return false;
        };
        if !self.ask_gate(cx).enabled {
            return false;
        }
        let k = &event.keystroke;
        let modified = k.modifiers.platform || k.modifiers.control;
        match self
            .asks
            .drafts
            .key(&row, &k.key, k.key_char.as_deref(), modified)
        {
            Keyed::Ignored => return false,
            Keyed::Changed => {}
            Keyed::Send(request) => {
                if let Some(request) = request {
                    self.send_ask(row.id, request, cx);
                }
            }
        }
        cx.notify();
        true
    }
}

fn frame(content: Div, a: Appearance) -> Div {
    div()
        .mx(px(SIDE_PADDING))
        .mb(px(8.0))
        .p(px(14.0))
        .rounded(px(RADIUS_COMPOSER))
        .fill(Ink::Card, a)
        .border(px(1.0))
        .border_color(hsla(Ink::Accent.color(a).with_alpha(0.45)))
        .child(content)
}

fn closed_line(text: &str, a: Appearance) -> Div {
    div()
        .mx(px(SIDE_PADDING))
        .mb(px(8.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(icon(Icon::Check, TypeRole::Label.spec().size, Ink::Done, a))
        .child(label(text, TypeRole::Label, Ink::TextMuted, a).elide())
}

fn choices(
    view: &ChatView,
    row: &AskRow,
    shown: &Card,
    gate: Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Vec<Stateful<Div>> {
    let held = Control {
        enabled: gate.enabled && !view.asks.drafts.busy(&row.id),
        tooltip: gate.tooltip,
    };
    shown
        .choices
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let element = option_row(
                SharedString::from(format!("ask-choice-{}-{i}", row.id)),
                i + 1,
                title,
                "",
                false,
                gate::state(held),
                a,
            );
            let id = row.id.clone();
            gate::tip(element.w_auto(), gate, a).when(held.enabled, |el| {
                el.on_click(cx.listener(move |this, _, _, cx| {
                    this.ask_act(&id, Act::Choose(i + 1), cx);
                }))
            })
        })
        .collect()
}

fn answer_box(
    view: &ChatView,
    row: &AskRow,
    gate: Control,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let typed = view.asks.drafts.typed(&row.id).to_owned();
    let focus = view.asks.focus.get(&row.id);
    let focused = focus.is_some_and(|handle| handle.is_focused(window));
    let element = div()
        .id(SharedString::from(format!("ask-answer-{}", row.id)))
        .flex_1()
        .min_w(px(0.0))
        .child(field(&typed, t::chat::ask_type_answer(), focused, a).w_full())
        .when(!gate.enabled, |b| b.opacity(0.4));
    let id = row.id.clone();
    let click_id = row.id.clone();
    let focus = focus.cloned();
    gate::tip(element, gate, a).when(gate.enabled, move |b| {
        let b = match &focus {
            Some(handle) => b.track_focus(handle),
            None => b,
        };
        b.cursor_text()
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(handle) = this.asks.focus.get(&click_id) {
                    window.focus(handle);
                }
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if this.ask_key(&id, event, cx) {
                    cx.stop_propagation();
                }
            }))
    })
}

fn open_card(
    view: &ChatView,
    row: &AskRow,
    shown: &Card,
    gate: Control,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Div {
    let busy = view.asks.drafts.busy(&row.id);
    let ready = Control {
        enabled: gate.enabled && !busy && !view.asks.drafts.typed(&row.id).trim().is_empty(),
        tooltip: gate.tooltip,
    };
    let open = Control {
        enabled: gate.enabled && !busy,
        tooltip: gate.tooltip,
    };
    let (send_id, dismiss_id) = (row.id.clone(), row.id.clone());
    let send = button(
        SharedString::from(format!("ask-send-{}", row.id)),
        t::chat::ask_send(),
        ButtonKind::Primary,
        gate::state(ready),
        a,
    );
    let send = gate::tip(send, ready, a).when(ready.enabled, |b| {
        b.on_click(cx.listener(move |this, _, _, cx| {
            this.ask_act(&send_id, Act::Send, cx);
        }))
    });
    let dismiss = button(
        SharedString::from(format!("ask-dismiss-{}", row.id)),
        t::chat::ask_dismiss(),
        ButtonKind::Ghost,
        gate::state(open),
        a,
    );
    let dismiss = gate::tip(dismiss, open, a).when(open.enabled, |b| {
        b.on_click(cx.listener(move |this, _, _, cx| {
            this.ask_act(&dismiss_id, Act::Dismiss, cx);
        }))
    });
    let lost = view
        .asks
        .drafts
        .lost(&row.id)
        .map(|line| label(line, TypeRole::Meta, Ink::Needs, a));
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .h(px(22.0))
                .flex()
                .items_center()
                .gap(px(7.0))
                .child(icon(
                    Icon::Question,
                    TypeRole::Heading.spec().size,
                    Ink::Working,
                    a,
                ))
                .child(label(&shown.header, TypeRole::Heading, Ink::Working, a).elide()),
        )
        .child(
            div()
                .id(SharedString::from(format!("ask-question-{}", row.id)))
                .max_h(px(QUESTION_MAX_HEIGHT))
                .overflow_y_scroll()
                .child(label(&shown.question, TypeRole::Heading, Ink::Text, a)),
        )
        .children(lost)
        .children(choices(view, row, shown, gate, a, cx))
        .child(
            div()
                .min_h(px(CONTROL))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(answer_box(view, row, gate, a, window, cx))
                .child(send)
                .child(dismiss),
        )
}

pub(super) fn cards(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Vec<Div> {
    let now = now_ms();
    let gate = view.ask_gate(cx);
    let mut out = Vec::new();
    let mut drawn = false;
    for row in view.ask_rows() {
        let Some(shown) = card(&row, now) else {
            continue;
        };
        drawn = true;
        let element = match &shown.closed_line {
            Some(line) => closed_line(line, a),
            None => frame(open_card(view, &row, &shown, gate, a, window, cx), a),
        };
        out.push(element.part_at("Ask Card", Corner::TopTrailing, a, cx));
    }
    view.asks
        .drawn_minute
        .set(drawn.then(|| now.div_euclid(MINUTE_MS)));
    out
}

#[cfg(test)]
#[path = "ask_cards_tests.rs"]
mod tests;
