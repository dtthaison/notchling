use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, icon, icon_button, KitPart, KitStyled};
use notchling_kit::{Icon, Ink, TypeRole};
use notchling_locale as t;

use super::blocking::{
    button_label, button_word, last_call, step_label, time_words, BlockingCard, Button, Closing,
    NoteRoute,
};
use super::cards::{text, A};
use super::layout::NotchState;
use super::model::NotchModel;
use super::view::NotchView;
use crate::acting::{Decision, Request};
use crate::attention::{BlockingTarget, Level};
use crate::chat::format::age;
use crate::chat::prompt_flow::{failure_text, Sending};
use crate::mac::gate;
use crate::menu::BlockAct;
use notchling_kit::ui::{editor_field, TextEditorEvent};

fn alarm(m: &NotchModel) -> Ink {
    if m.level == Level::Calm {
        Ink::TextMuted
    } else {
        Ink::Expired
    }
}

fn red_dot(ink: Ink) -> Div {
    div()
        .size(px(7.0))
        .flex_shrink_0()
        .rounded_full()
        .fill(ink, A)
}

fn count_pill(count: usize, ink: Ink) -> Div {
    div()
        .min_w(px(16.0))
        .h(px(16.0))
        .px(px(4.0))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded_full()
        .fill(ink, A)
        .child(text(count.to_string(), TypeRole::Meta, Ink::OnAccent))
}

fn kind(button: Button) -> ButtonKind {
    match button {
        Button::Allow | Button::AllowOnce => ButtonKind::Primary,
        Button::Later => ButtonKind::Ghost,
        _ => ButtonKind::Secondary,
    }
}

impl NotchView {
    pub(super) fn blocking_ears(&self, cx: &mut Context<Self>) -> Option<(Div, Stateful<Div>)> {
        let m = &self.model;
        let ink = alarm(m);
        let Some(card) = m.blocking.current() else {
            let count = m.closed_count()?;
            let left = div().flex().items_center().child(red_dot(Ink::Needs));
            let right = div()
                .id("notch-waiting-count")
                .flex()
                .items_center()
                .cursor_pointer()
                .child(count_pill(count.count, Ink::Needs))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.model.go(NotchState::List);
                    cx.notify();
                }));
            return Some((left, right));
        };
        let key = card.id.clone();
        let wait = age(m.now_ms, card.since_ms);
        let left = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(red_dot(ink))
            .child(text(wait, TypeRole::Meta, ink));
        let right = div()
            .id("notch-blocking-count")
            .flex()
            .items_center()
            .cursor_pointer()
            .child(count_pill(m.blocking.count(), ink))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.open_blocking(&key, 0, true, this.model.now_ms);
                cx.notify();
            }));
        Some((left, right))
    }

    pub(super) fn blocking_glance(&self) -> Div {
        let m = &self.model;
        let ink = alarm(m);
        if let Some((title, word)) = m.shown_farewell() {
            return div()
                .h_full()
                .flex()
                .items_center()
                .px(px(14.0))
                .gap(px(8.0))
                .child(
                    text(title, TypeRole::Body, Ink::TextMuted)
                        .flex_1()
                        .min_w(px(0.0))
                        .elide(),
                )
                .child(text(word, TypeRole::Body, Ink::Text).flex_shrink_0());
        }
        let line = m.blocking.current().map(|card| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(14.0))
                .child(red_dot(ink))
                .child(
                    text(card.title.clone(), TypeRole::Body, Ink::Text)
                        .flex_1()
                        .min_w(px(0.0))
                        .elide(),
                )
                .child(text(time_words(card, m.now_ms), TypeRole::Meta, ink).flex_shrink_0())
        });
        div().h_full().flex().items_center().children(line)
    }

    fn step_button(&self, forward: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let m = &self.model;
        let (id, glyph) = if forward {
            ("notch-blocking-next", Icon::ChevronRight)
        } else {
            ("notch-blocking-prev", Icon::ChevronLeft)
        };
        let button = icon_button(id, glyph, ButtonKind::Ghost, PartState::Rest, A).on_click(
            cx.listener(move |this, _, _, cx| {
                this.model.step_blocking(forward, this.model.now_ms);
                cx.notify();
            }),
        );
        let label = step_label(forward, m.blocking.position(), m.blocking.count());
        gate::tip_text(button, SharedString::from(label), A)
    }

    fn blocking_head(&self, card: &BlockingCard, cx: &mut Context<Self>) -> Div {
        let m = &self.model;
        let ink = alarm(m);
        let total = m.blocking.count();
        let title = if last_call(card, m.now_ms) {
            t::notch::blocking_ends_soon().to_owned()
        } else if total > 1 {
            t::notch::blocking_title_count(total as i64)
        } else {
            t::notch::blocking_title().to_owned()
        };
        let stepper = (total > 1).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(2.0))
                .child(self.step_button(false, cx))
                .child(text(
                    t::notch::blocking_step((m.blocking.position() + 1) as i64, total as i64),
                    TypeRole::Meta,
                    Ink::TextMuted,
                ))
                .child(self.step_button(true, cx))
        });
        div()
            .h(px(super::layout::BLOCKING_HEAD as f32))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(red_dot(ink))
            .child(text(title, TypeRole::Heading, ink).flex_shrink_0())
            .children(stepper)
            .child(div().flex_1())
            .child(
                text(time_words(card, m.now_ms), TypeRole::Meta, ink)
                    .min_w(px(0.0))
                    .elide(),
            )
    }

    pub(super) fn blocking_panel(&self, cx: &mut Context<Self>) -> Div {
        let m = &self.model;
        let frame = div().px(px(12.0)).pt(px(4.0)).flex().flex_col();
        if let Some((title, word)) = m.shown_farewell() {
            return frame
                .child(text(title, TypeRole::Heading, Ink::TextMuted).elide())
                .child(text(word, TypeRole::Heading, Ink::Text));
        }
        let Some(card) = m.blocking.current().cloned() else {
            return frame;
        };
        frame
            .child(self.blocking_head(&card, cx))
            .child(self.item_body(&card, cx))
            .part_inside(
                t::notch::blocking_part(),
                notchling_kit::labels::Corner::BottomTrailing,
                A,
                cx,
            )
    }

    fn item_body(&self, card: &BlockingCard, cx: &mut Context<Self>) -> Div {
        let m = &self.model;
        let ink = alarm(m);
        let who = (!card.who.is_empty()).then(|| {
            let joined = card.who.join(" · ");
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon(Icon::Stop, 12.0, Ink::TextMuted, A))
                .child(
                    text(
                        t::notch::blocking_who(&joined),
                        TypeRole::Meta,
                        Ink::TextMuted,
                    )
                    .flex_1()
                    .min_w(px(0.0))
                    .elide(),
                )
        });
        let clock = last_call(card, m.now_ms).then(|| {
            div()
                .flex()
                .items_baseline()
                .gap(px(8.0))
                .child(text(time_words(card, m.now_ms), TypeRole::Title, ink))
                .child(
                    text(t::notch::blocking_left(), TypeRole::Meta, Ink::TextMuted)
                        .min_w(px(0.0))
                        .elide(),
                )
        });
        let detail = (!card.detail.is_empty()).then(|| {
            if card.held {
                text(card.detail.clone(), TypeRole::Mono, Ink::Text)
                    .line_clamp(3)
                    .p(px(8.0))
                    .rounded(px(RADIUS_MD))
                    .fill(Ink::Raised, A)
            } else {
                text(card.detail.clone(), TypeRole::Meta, Ink::TextMuted).line_clamp(3)
            }
        });
        let noting = m.blocking.note_open() == Some(card.id.as_str());
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .children(clock)
            .child(text(card.title.clone(), TypeRole::Heading, Ink::Text).line_clamp(2))
            .children(detail)
            .children(who)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .w_full()
                    .min_w(px(0.0))
                    .items_center()
                    .gap(px(8.0))
                    .p(px(0.0))
                    .rounded(px(RADIUS_MD))
                    .children(
                        card.buttons
                            .iter()
                            .map(|b| self.blocking_button(card, *b, cx)),
                    ),
            )
            .children(noting.then(|| self.note_row(card, cx)))
            .children(
                self.send_error
                    .as_ref()
                    .map(|error| text(error.clone(), TypeRole::Meta, Ink::Needs).line_clamp(2)),
            )
    }

    fn note_row(&self, card: &BlockingCard, cx: &mut Context<Self>) -> Div {
        let id = card.id.clone();
        let has_text = !self.model.blocking.draft(&card.id).trim().is_empty()
            && !self.model.blocking.busy(&card.id);
        let send = button(
            "notch-blocking-note-send",
            t::consent::change_send(),
            ButtonKind::Primary,
            if has_text {
                PartState::Rest
            } else {
                PartState::Disabled
            },
            A,
        );
        let send = send.when(has_text, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| this.send_note(&id, cx)))
        });
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(editor_field(self.note.editor(), self.note_focused, A).w_full()),
            )
            .child(send)
    }

    pub(super) fn note_event(&mut self, event: &TextEditorEvent, cx: &mut Context<Self>) {
        let Some(id) = self.model.blocking.note_open().map(str::to_owned) else {
            return;
        };
        let typed = self.note.editor().read(cx).text().to_owned();
        self.model.blocking.set_draft(&id, &typed);
        let kept = self.model.blocking.draft(&id).to_owned();
        if kept != typed {
            self.note
                .editor()
                .update(cx, |editor, cx| editor.set_text(&kept, cx));
        }
        if matches!(event, TextEditorEvent::Submit) {
            self.send_note(&id, cx);
        }
        cx.notify();
    }

    pub(super) fn sync_note(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let open = self.model.blocking.note_open().map(str::to_owned);
        if open != self.note_at {
            self.note_at = open.clone();
            let text = open
                .as_deref()
                .map(|id| self.model.blocking.draft(id).to_owned())
                .unwrap_or_default();
            let editor = self.note.editor().clone();
            editor.update(cx, |editor, cx| {
                editor.set_text(&text, cx);
                if open.is_some() {
                    editor.focus(window);
                }
            });
        }
        self.note_focused = self.note.editor().read(cx).is_focused(window);
    }

    fn send_note(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some((route, note)) = self.model.blocking.note_to_send(id) else {
            return;
        };
        if self.sending.is_some() || self.model.blocking.busy(id) {
            return;
        }
        let id = id.to_owned();
        match route {
            NoteRoute::Hook(prompt) => {
                if !self.acting().control().enabled {
                    return;
                }
                self.model.blocking.note_answer(&id, Closing::Changed);
                self.note_for = Some((prompt.clone(), id));
                let request = Request::permission_deny_with(&prompt, &note);
                let sent = Sending::Decision {
                    decision: Decision::Deny,
                    on_screen: false,
                };
                self.send_unless_off(Some(prompt), request, sent, cx);
            }
            route => {
                let Some(task) = crate::mac::shell::blocking_note(route, note, cx) else {
                    return;
                };
                if !self.model.blocking.begin(&id) {
                    return;
                }
                self.model.blocking.note_answer(&id, Closing::Changed);
                self.send_error = None;
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    this.update(cx, |view, cx| {
                        let sent = matches!(result, Ok(true));
                        view.model.blocking.finish(&id, sent);
                        view.model.blocking.note_settled(&id, sent);
                        if !sent {
                            view.send_error = Some(match result {
                                Err(message) => failure_text(&message),
                                Ok(_) => t::notch::blocking_note_kept().to_owned(),
                            });
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
        }
        cx.notify();
    }

    fn act(&mut self, id: &str, act: BlockAct, how: Closing, cx: &mut Context<Self>) {
        if !self.model.blocking.begin(id) {
            return;
        }
        self.model.blocking.note_answer(id, how);
        let task = crate::mac::shell::blocking_act(act, id, cx);
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            let answered = task.await;
            this.update(cx, |view, cx| {
                view.model.blocking.finish(&id, answered);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn blocking_button(
        &self,
        card: &BlockingCard,
        which: Button,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let ready = self
            .model
            .blocking
            .pressable(&card.id, which, self.model.now_ms);
        let state = if ready {
            PartState::Rest
        } else {
            PartState::Disabled
        };
        let id = card.id.clone();
        let target = card.target.clone();
        let session = card.sessions.first().cloned();
        let element = button(
            SharedString::from(format!("notch-blocking-{which:?}")),
            button_word(which),
            kind(which),
            state,
            A,
        );
        let element = gate::tip_text(element, SharedString::from(button_label(card, which)), A);
        if state == PartState::Disabled {
            return element;
        }
        element.on_click(cx.listener(move |this, _, _, cx| {
            match (which, &target) {
                (Button::Later, _) => this.model.close_blocking(),
                (Button::Change, _) => {
                    if this.model.blocking.note_open() == Some(id.as_str()) {
                        this.model.blocking.close_note();
                    } else {
                        this.model.blocking.open_note(&id);
                    }
                }
                (Button::Open | Button::OpenForm, BlockingTarget::HookPrompt(_)) => {
                    if let Some(target) = session
                        .as_deref()
                        .and_then(crate::search::wire::SessionRef::parse)
                    {
                        crate::mac::shell::open_chat_session(cx, target, None);
                    }
                }
                (Button::Allow, BlockingTarget::HookPrompt(prompt)) => {
                    this.model.blocking.note_answer(&id, Closing::Allowed);
                    let request = Request::permission(prompt, Decision::Allow);
                    let sent = Sending::Decision {
                        decision: Decision::Allow,
                        on_screen: false,
                    };
                    this.send_for(Some(prompt.clone()), request, sent, cx);
                }
                (Button::Deny, BlockingTarget::HookPrompt(prompt)) => {
                    this.model.blocking.note_answer(&id, Closing::Denied);
                    let request = Request::permission(prompt, Decision::Deny);
                    let sent = Sending::Decision {
                        decision: Decision::Deny,
                        on_screen: false,
                    };
                    this.send_unless_off(Some(prompt.clone()), request, sent, cx);
                }
                (_, BlockingTarget::HookPrompt(_)) => {}
                (Button::Allow | Button::AllowOnce, _) => {
                    this.act(&id, BlockAct::Allow, Closing::Allowed, cx);
                }
                (Button::Deny, _) => {
                    this.act(&id, BlockAct::Deny, Closing::Denied, cx);
                }
                (Button::Open | Button::OpenForm, _) => {
                    crate::mac::shell::blocking_act(BlockAct::OpenInbox, &id, cx).detach()
                }
            }
            cx.notify();
        }))
    }
}
