use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_engine::transcript::snapshot::PendingQuestion;
use notchling_kit::metric::{CONTROL, OPTION_BADGE, RADIUS_MD, RADIUS_SM};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, field, hsla, icon as kit_icon, option_row, KitStyled};
use notchling_kit::{sentence, Icon, Ink, TypeRole};

use super::answer::{type_key, QuestionDraft, Typed};
use super::cards::{line, text, A};
use super::requests::HookRequest;
use super::view::NotchView;
use crate::acting::{Control, Decision, Request};
use crate::chat::prompt_flow::{failure_text, sending_text, Sending};
use crate::mac::gate;

pub(super) const HEADER: f32 = 22.0;
pub(super) const GAP: f32 = 8.0;
const TAB_ROW: f32 = 24.0;
const OPTION_PADDING: f32 = 20.0;
const CHARS_PER_LINE: usize = 52;
const MONO_CHARS_PER_LINE: usize = 48;
const SUMMARY_LINES: usize = 10;

fn wrapped_lines(chars: usize, per_line: usize) -> f32 {
    (chars / per_line + 1) as f32
}

fn option_height() -> f32 {
    let text = line(TypeRole::Heading) + 2.0 + line(TypeRole::Meta);
    text.max(OPTION_BADGE) + OPTION_PADDING
}

const META_CHARS_PER_LINE: usize = 62;

pub(super) fn status_height(status: Option<&str>) -> f32 {
    match status {
        Some(t) => {
            wrapped_lines(t.chars().count(), META_CHARS_PER_LINE) * line(TypeRole::Meta) + GAP
        }
        None => 0.0,
    }
}

pub fn question_height(q: &PendingQuestion, tab: usize, status: Option<&str>) -> f32 {
    let Some(item) = q.items.get(tab).or_else(|| q.items.first()) else {
        return 0.0;
    };
    let tabs = if q.items.len() > 1 {
        TAB_ROW + GAP
    } else {
        0.0
    };
    let question = wrapped_lines(item.question.chars().count(), CHARS_PER_LINE).min(3.0)
        * line(TypeRole::Heading);
    let hint = if item.multi_select {
        line(TypeRole::Meta) + GAP
    } else {
        0.0
    };
    let options = (option_height() + GAP) * item.options.len() as f32;
    let submit = if QuestionDraft::needs_submit(q) {
        CONTROL + GAP
    } else {
        0.0
    };
    HEADER
        + GAP
        + tabs
        + question
        + GAP
        + hint
        + options
        + CONTROL
        + GAP
        + submit
        + status_height(status)
}

pub fn permission_height(r: &HookRequest, status: Option<&str>) -> f32 {
    let lines = wrapped_lines(r.summary.chars().count(), MONO_CHARS_PER_LINE)
        + r.summary.matches('\n').count() as f32;
    let summary = lines.min(SUMMARY_LINES as f32) * line(TypeRole::Mono) + 16.0;
    let count = if r.can_always_allow { 3 } else { 2 };
    HEADER
        + GAP
        + line(TypeRole::Heading)
        + GAP
        + summary
        + GAP
        + (option_height() + GAP) * count as f32
        + status_height(status)
}

fn header(icon: Icon, title: &str, ink: Ink, step: Option<String>, agent: Option<&str>) -> Div {
    div()
        .h(px(HEADER))
        .flex()
        .items_center()
        .gap(px(7.0))
        .child(kit_icon(icon, TypeRole::Heading.spec().size, ink, A))
        .child(text(title.to_owned(), TypeRole::Heading, ink))
        .children(step.map(|s| text(s, TypeRole::Meta, Ink::TextMuted)))
        .child(div().flex_1())
        .children(agent.map(|name| text(name.to_owned(), TypeRole::Meta, Ink::TextMuted).elide()))
}

impl NotchView {
    pub(super) fn card_status(&self, gate: Control) -> Option<Div> {
        let ink = if self.send_error.is_some() {
            Ink::Needs
        } else {
            Ink::TextMuted
        };
        self.status_text(gate)
            .map(|t| text(sentence(t), TypeRole::Meta, ink))
    }

    pub(super) fn status_text(&self, gate: Control) -> Option<&str> {
        if let Some(error) = &self.send_error {
            return Some(error);
        }
        if let Some(sending) = self.sending {
            return Some(sending_text(sending));
        }
        gate.tooltip
    }

    pub(super) fn send_for(
        &mut self,
        key: Option<String>,
        request: Request,
        sending: Sending,
        cx: &mut Context<Self>,
    ) {
        if self.sending.is_some() || !self.gate().enabled {
            return;
        }
        self.sending = Some(sending);
        self.send_error = None;
        let actions = self.actions.clone();
        let shared = self.sessions.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.sending = None;
                match result {
                    Ok(_) => {
                        if let Some(key) = &key {
                            view.model.mark_answered_key(key);
                            if let Some(sessions) = &shared {
                                sessions.mark_answered(key);
                            }
                        }
                        view.fold_peek();
                    }
                    Err(message) => view.send_error = Some(failure_text(&message)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn question_card(
        &self,
        id: &str,
        q: &PendingQuestion,
        agent: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let gate = self.gate();
        let tab = self.draft.tab;
        let Some(item) = q.items.get(tab) else {
            return div();
        };
        let step = (q.items.len() > 1).then(|| format!("{} of {}", tab + 1, q.items.len()));
        let tabs = (q.items.len() > 1).then(|| self.question_tabs(q, cx));
        let options = item
            .options
            .iter()
            .enumerate()
            .map(|(i, title)| {
                let detail = item.details.get(i).map(String::as_str).unwrap_or("");
                let picked = self.draft.is_picked(tab, i + 1);
                let row = option_row(
                    SharedString::from(format!("option-{tab}-{i}")),
                    i + 1,
                    title,
                    detail,
                    picked,
                    gate::state(gate),
                    A,
                )
                .line_height(px(line(TypeRole::Heading)));
                let (session, question) = (id.to_owned(), q.clone());
                gate::tip(row, gate, A).when(gate.enabled, |row| {
                    row.on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(answers) = this.draft.choose(&question, i + 1) {
                            let key = this.model.answer_key(&session);
                            this.send_for(
                                key,
                                Request::answer_for(&session, &question, &answers),
                                Sending::Answer,
                                cx,
                            );
                        }
                        cx.notify();
                    }))
                })
            })
            .collect::<Vec<_>>();
        let hint = item
            .multi_select
            .then(|| text("Pick any that apply", TypeRole::Meta, Ink::TextMuted));
        let submit = QuestionDraft::needs_submit(q).then(|| self.submit_row(id, q, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(header(
                Icon::Question,
                "Claude asks",
                Ink::Working,
                step,
                agent,
            ))
            .children(tabs)
            .child(text(item.question.clone(), TypeRole::Heading, Ink::Text).line_clamp(3))
            .children(hint)
            .children(options)
            .child(self.answer_box(id, q, cx))
            .children(submit)
            .children(self.card_status(gate))
    }

    fn question_tabs(&self, q: &PendingQuestion, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(TAB_ROW))
            .flex()
            .items_center()
            .gap(px(4.0))
            .children(self.draft.tabs(q).into_iter().enumerate().map(|(i, tab)| {
                let answered = tab.answered;
                let title = tab.title;
                let ink = if tab.selected {
                    Ink::Text
                } else if tab.answered {
                    Ink::Done
                } else {
                    Ink::TextMuted
                };
                div()
                    .id(SharedString::from(format!("question-tab-{i}")))
                    .h(px(TAB_ROW))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .rounded_full()
                    .cursor_pointer()
                    .when(tab.selected, |t| t.fill(Ink::Raised, A))
                    .when(answered, |t| t.child(kit_icon(Icon::Check, 10.0, ink, A)))
                    .child(text(title, TypeRole::Label, ink).elide())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.draft.select_tab(i);
                        cx.notify();
                    }))
            }))
    }

    fn answer_box(&self, id: &str, q: &PendingQuestion, cx: &mut Context<Self>) -> Stateful<Div> {
        let gate = self.gate();
        let tab = self.draft.tab;
        let typed = self.draft.typed(tab).to_owned();
        let shown = match self.draft.text_answer(tab) {
            Some(answer) if typed.is_empty() => answer.to_owned(),
            _ => typed,
        };
        let focused = self.focused;
        let (session, question) = (id.to_owned(), q.clone());
        let element = div()
            .id(SharedString::from(format!("answer-box-{tab}")))
            .child(field(&shown, "Type something else…", focused, A).w_full())
            .when(!gate.enabled, |b| b.opacity(0.4));
        gate::tip(element, gate, A).when(gate.enabled, |b| {
            b.track_focus(&self.focus)
                .cursor_text()
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus);
                    cx.notify();
                }))
                .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                    let k = &event.keystroke;
                    let modified = k.modifiers.platform || k.modifiers.control;
                    let Some(typed) = this.draft.typed_mut() else {
                        return;
                    };
                    match type_key(typed, &k.key, k.key_char.as_deref(), modified) {
                        Typed::Submit => {
                            if let Some(answers) = this.draft.submit_text(&question) {
                                let request = Request::answer_for(&session, &question, &answers);
                                let key = this.model.answer_key(&session);
                                this.send_for(key, request, Sending::Answer, cx);
                            }
                        }
                        Typed::Changed => {}
                        Typed::Ignored => return,
                    }
                    cx.stop_propagation();
                    cx.notify();
                }))
        })
    }

    fn submit_row(&self, id: &str, q: &PendingQuestion, cx: &mut Context<Self>) -> Div {
        let gate = self.gate();
        let ready = Control {
            enabled: gate.enabled && self.draft.complete() && self.sending.is_none(),
            tooltip: gate.tooltip,
        };
        let multi = q.items.get(self.draft.tab).is_some_and(|i| i.multi_select);
        let next = (multi && q.items.len() > 1).then(|| {
            button(
                "question-next",
                "Next",
                ButtonKind::Secondary,
                PartState::Rest,
                A,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.draft.next();
                cx.notify();
            }))
        });
        let (session, question) = (id.to_owned(), q.clone());
        let submit = button(
            "question-submit",
            "Submit",
            ButtonKind::Primary,
            gate::state(ready),
            A,
        );
        let submit = gate::tip(submit, ready, A).when(ready.enabled, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| {
                if let Some(answers) = this.draft.answers(&question) {
                    let key = this.model.answer_key(&session);
                    this.send_for(
                        key,
                        Request::answer_for(&session, &question, &answers),
                        Sending::Answer,
                        cx,
                    );
                }
            }))
        });
        div()
            .h(px(CONTROL))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.0))
            .children(next)
            .child(submit)
    }

    pub(super) fn permission_card(
        &self,
        r: &HookRequest,
        agent: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Div {
        if let Some(agy) = &r.agy {
            return self.agy_permission_card(r, agy, agent, cx);
        }
        let gate = self.gate();
        let mut choices = vec![(Decision::Allow, "Allow", "This time only")];
        if r.can_always_allow {
            choices.push((
                Decision::Always,
                "Always allow this",
                "Don't ask again for this",
            ));
        }
        choices.push((Decision::Deny, "Deny", "Claude tries something else"));
        let sending = self.sending;
        let chosen = match sending {
            Some(Sending::Decision { decision, .. }) => Some(decision),
            _ => None,
        };
        let options = choices
            .into_iter()
            .enumerate()
            .map(|(i, (decision, title, detail))| {
                let (state, picked) = match sending {
                    None => (gate::state(gate), false),
                    Some(_) if chosen == Some(decision) => (PartState::Selected, true),
                    Some(_) => (PartState::Disabled, false),
                };
                let row = option_row(
                    SharedString::from(format!("permission-{i}")),
                    i + 1,
                    title,
                    detail,
                    picked,
                    state,
                    A,
                )
                .line_height(px(line(TypeRole::Heading)));
                let prompt = r.id.clone();
                let sent = Sending::Decision {
                    decision,
                    on_screen: false,
                };
                gate::tip(row, gate, A).when(gate.enabled && sending.is_none(), |row| {
                    row.on_click(cx.listener(move |this, _, _, cx| {
                        let request = Request::permission(&prompt, decision);
                        this.send_for(Some(prompt.clone()), request, sent, cx);
                    }))
                })
            });
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(header(
                Icon::Permission,
                "Permission needed",
                Ink::AccentText,
                None,
                agent,
            ))
            .child(text(
                format!("Claude wants to use {}", r.tool_name),
                TypeRole::Heading,
                Ink::Text,
            ))
            .child(
                text(r.summary.clone(), TypeRole::Mono, Ink::Text)
                    .line_clamp(SUMMARY_LINES)
                    .p(px(8.0))
                    .rounded(px(RADIUS_MD))
                    .fill(Ink::Raised, A),
            )
            .children(options)
            .children(self.card_status(gate))
    }
}

pub fn badge(count: usize) -> Div {
    div()
        .min_w(px(15.0))
        .h(px(15.0))
        .px(px(4.0))
        .rounded(px(RADIUS_SM * 2.0))
        .bg(hsla(Ink::Needs.color(A)))
        .flex()
        .items_center()
        .justify_center()
        .kit_type(TypeRole::Meta)
        .text_size(px(10.0))
        .ink(Ink::OnAccent, A)
        .child(count.to_string())
}
