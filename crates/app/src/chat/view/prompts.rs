use notchling_locale as t;
use std::collections::HashMap;

use std::time::Duration;

use gpui::{
    div, prelude::*, px, Context, Div, FocusHandle, KeyDownEvent, SharedString, Stateful, Window,
};
use notchling_engine::transcript::snapshot::PendingQuestion;
use notchling_kit::labels::Corner;
use notchling_kit::metric::{CONTROL, MENU_GAP, RADIUS_COMPOSER, RADIUS_MD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, code_block, field, hsla, icon, label, option_row, KitPart, KitStyled, MenuItem,
};
use notchling_kit::{sentence, Appearance, Icon, Ink, TypeRole};

use super::ChatView;
use crate::acting::{Control, Decision, Request};
use crate::chat::data::Tool;
use crate::chat::permission_text;
use crate::chat::prompt_flow::{
    confirmation, failure_text, file_name, is_gone, sending_text, Confirmed, Sending, Told,
    CONFIRM_MS,
};
use crate::chat::prompts::{
    is_dialog, permission_request, preview_prompt, prompt_for, question_key, screen_question,
    screen_read_key, Preview, Prompt, NEEDS_PREFIX,
};
use crate::mac::gate;
use crate::notch::answer::{type_key, QuestionDraft, Typed};
use crate::notch::requests::HookRequest;
use crate::phone::bridge::now_ms;

const SCREEN_RETRY_MS: i64 = 3_000;

pub struct State {
    pub shown: Option<Prompt>,
    pub files: HashMap<String, HookRequest>,
    pub transcript: Option<PendingQuestion>,
    screen: Option<(String, PendingQuestion)>,
    screen_asked: Option<String>,
    screen_retry_ms: i64,
    draft: QuestionDraft,
    sending: Option<Sending>,
    error: Option<String>,
    command_open: Option<String>,
    confirmed: Option<(String, Confirmed)>,
    focus: FocusHandle,
    pub(super) preview: Option<Preview>,
}

impl State {
    pub fn new(preview: Option<Preview>, cx: &mut Context<ChatView>) -> State {
        State {
            shown: None,
            files: HashMap::new(),
            transcript: None,
            screen: None,
            screen_asked: None,
            screen_retry_ms: 0,
            draft: QuestionDraft::default(),
            sending: None,
            error: None,
            command_open: None,
            confirmed: None,
            focus: cx.focus_handle(),
            preview,
        }
    }

    pub fn is_shown(&self) -> bool {
        self.shown.is_some()
    }

    pub fn apply_preview(&mut self, preview: Preview) {
        use crate::chat::prompts::PREVIEW_BEFORE;
        self.preview = Some(preview);
        self.shown = Some(preview_prompt(preview));
        match preview {
            Preview::PermissionExpanded => {
                self.command_open = Some("preview".into());
            }
            Preview::PermissionSending => {
                self.sending = Some(Sending::Decision {
                    decision: Decision::Allow,
                    on_screen: true,
                });
            }
            Preview::PermissionFailed => {
                self.error = Some(failure_text(
                    "The terminal asks about something else than Chat shows, so nothing was typed. Answer it in the terminal.",
                ));
            }
            Preview::PermissionNext => {
                self.confirmed = Some((
                    String::new(),
                    Confirmed {
                        text: confirmation("Write", PREVIEW_BEFORE, Decision::Allow),
                        until_ms: i64::MAX,
                    },
                ));
            }
            _ => {}
        }
    }
}

impl ChatView {
    fn prompt_now(&self) -> Option<Prompt> {
        let session = self.selected_session()?;
        if let Some(preview) = self.prompt.preview {
            return (session.tool == Tool::Claude).then(|| preview_prompt(preview));
        }
        let (prompts, answered) = self.source.held();
        let key = screen_read_key(&session.agent, session.tool);
        let screen = self
            .prompt
            .screen
            .as_ref()
            .filter(|(k, _)| key.as_ref() == Some(k))
            .map(|(_, q)| q);
        prompt_for(
            &session.agent,
            session.tool,
            prompts.as_deref(),
            &self.prompt.files,
            self.prompt.transcript.as_ref(),
            screen,
            &answered,
        )
    }

    fn read_screen_question(&mut self, cx: &mut Context<Self>) {
        if self.prompt.preview.is_some() || self.root.fixture.is_some() {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(key) = screen_read_key(&session.agent, session.tool) else {
            return;
        };
        let row = session.agent.id.clone();
        let held = matches!(&self.prompt.shown, Some(Prompt::Question(q)) if q.tool_use_id.starts_with("hook:"));
        if held
            || self.prompt.transcript.is_some()
            || (self.prompt.screen_asked.as_ref() == Some(&key)
                && (self.prompt.screen.as_ref().is_some_and(|(k, _)| *k == key)
                    || now_ms() < self.prompt.screen_retry_ms))
            || !self.acting(cx).control().enabled
        {
            return;
        }
        self.prompt.screen_asked = Some(key.clone());
        self.prompt.screen_retry_ms = i64::MAX;
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&Request::question(&row)) })
                .await;
            this.update(cx, |view, cx| {
                match result.ok().as_ref().and_then(screen_question) {
                    Some(q) => view.prompt.screen = Some((key, q)),
                    None => view.prompt.screen_retry_ms = now_ms() + SCREEN_RETRY_MS,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn sync_prompt(&mut self, cx: &mut Context<Self>) {
        self.read_screen_question(cx);
        let shown = self.prompt_now();
        let key = |p: &Option<Prompt>| p.as_ref().map(Prompt::key);
        if self.prompt.preview.is_none() && key(&shown) != key(&self.prompt.shown) {
            self.prompt.error = None;
        }
        if let Some(Prompt::Question(q)) = &shown {
            self.prompt.draft.follow(q);
        }
        self.prompt.shown = shown;
    }

    fn prompt_gate(&self, cx: &mut Context<Self>) -> Control {
        if self.prompt.preview.is_some() {
            return Control {
                enabled: true,
                tooltip: None,
            };
        }
        self.acting(cx).control()
    }

    fn answer_prompt(
        &mut self,
        key: String,
        request: Request,
        sending: Sending,
        told: Option<Told>,
        cx: &mut Context<Self>,
    ) {
        if self.prompt.sending.is_some() || !self.prompt_gate(cx).enabled {
            return;
        }
        self.prompt.sending = Some(sending);
        self.prompt.error = None;
        let row = self.selected.clone();
        self.follow_sent_now(cx);
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.prompt.sending = None;
                match result {
                    Ok(_) => {
                        view.source.mark_answered(&key);
                        if let (Some(told), Some(row)) = (&told, &row) {
                            view.keep_note(row, told.went_through.clone(), cx);
                        }
                        if view.tools.error.as_deref().is_some_and(is_dialog) {
                            view.tools.error = None;
                        }
                        view.reload_now(cx);
                    }
                    Err(message) => match (&told, &row) {
                        (Some(told), Some(row)) if is_gone(&message) => {
                            view.source.mark_answered(&key);
                            view.keep_note(row, told.already_answered.clone(), cx);
                            view.reload_now(cx);
                        }
                        _ => view.prompt.error = Some(failure_text(&message)),
                    },
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn keep_note(&mut self, row: &str, text: String, cx: &mut Context<Self>) {
        self.prompt.confirmed = Some((row.to_owned(), Confirmed::new(text, now_ms())));
        self.draw_when_confirmation_ends(cx);
    }

    fn draw_when_confirmation_ends(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(CONFIRM_MS as u64 + 50))
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }
}

fn status(view: &ChatView, gate: Control, a: Appearance) -> Option<Div> {
    let text = match view.prompt.sending {
        Some(sending) => sending_text(sending),
        None => gate.tooltip?,
    };
    Some(label(&sentence(text), TypeRole::Meta, Ink::TextMuted, a))
}

fn failed(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let message = view.prompt.error.as_deref()?;
    Some(
        div()
            .flex()
            .items_start()
            .gap(px(8.0))
            .p(px(8.0))
            .rounded(px(RADIUS_MD))
            .fill(Ink::Raised, a)
            .child(icon(
                Icon::Warning,
                TypeRole::Label.spec().size,
                Ink::Needs,
                a,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap(px(2.0))
                    .child(label(
                        t::chat::not_answered(),
                        TypeRole::Label,
                        Ink::Needs,
                        a,
                    ))
                    .child(label(message, TypeRole::Meta, Ink::Text, a)),
            )
            .part_at("Answer Error", Corner::TopTrailing, a, cx),
    )
}

fn confirmed(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let (row, done) = view.prompt.confirmed.as_ref()?;
    let for_this_session = view.prompt.preview.is_some() || view.selected.as_ref() == Some(row);
    if !for_this_session || !done.visible(now_ms()) {
        return None;
    }
    Some(
        div()
            .mx(px(SIDE_PADDING))
            .mb(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(icon(Icon::Check, TypeRole::Label.spec().size, Ink::Done, a))
            .child(label(&done.text, TypeRole::Label, Ink::Text, a).elide())
            .part_at("Answer Confirmation", Corner::TopTrailing, a, cx),
    )
}

fn header(icon_: Icon, title: &str, ink: Ink, step: Option<String>, a: Appearance) -> Div {
    div()
        .h(px(22.0))
        .flex()
        .items_center()
        .gap(px(7.0))
        .child(icon(icon_, TypeRole::Heading.spec().size, ink, a))
        .child(label(title, TypeRole::Heading, ink, a))
        .children(step.map(|s| label(&s, TypeRole::Meta, Ink::TextMuted, a)))
}

fn frame(prompt: &Prompt, content: Div, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    div()
        .mx(px(SIDE_PADDING))
        .mb(px(8.0))
        .p(px(14.0))
        .rounded(px(RADIUS_COMPOSER))
        .fill(Ink::Card, a)
        .border(px(1.0))
        .border_color(hsla(Ink::Accent.color(a).with_alpha(0.45)))
        .child(content)
        .part_at(prompt.part(), Corner::TopTrailing, a, cx)
}

pub(super) fn card(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<Div> {
    let done = confirmed(view, a, cx);
    let Some(prompt) = view.prompt.shown.clone() else {
        return done;
    };
    let row = view.selected.clone()?;
    let gate = view.prompt_gate(cx);
    let content = match &prompt {
        Prompt::Question(q) => question(view, &row, q, gate, a, window, cx),
        Prompt::Permission(r) => permission(view, r, gate, a, cx),
    };
    Some(
        div()
            .flex()
            .flex_col()
            .children(done)
            .child(frame(&prompt, content, a, cx)),
    )
}

fn question(
    view: &ChatView,
    row: &str,
    q: &PendingQuestion,
    gate: Control,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Div {
    let draft = &view.prompt.draft;
    let tab = draft.tab;
    let Some(item) = q.items.get(tab) else {
        return div();
    };
    let step = (q.items.len() > 1).then(|| t::chat::step_of(tab + 1, q.items.len()));
    let tabs = (q.items.len() > 1).then(|| tabs(view, q, a, cx));
    let options = item
        .options
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let detail = item.details.get(i).map(String::as_str).unwrap_or("");
            let row_el = option_row(
                SharedString::from(format!("prompt-option-{tab}-{i}")),
                i + 1,
                title,
                detail,
                draft.is_picked(tab, i + 1),
                gate::state(gate),
                a,
            );
            let (session, question) = (row.to_owned(), q.clone());
            gate::tip(row_el.w_auto(), gate, a).when(gate.enabled, |el| {
                el.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(answers) = this.prompt.draft.choose(&question, i + 1) {
                        let request = Request::answer_for(&session, &question, &answers);
                        this.answer_prompt(
                            question_key(&question),
                            request,
                            Sending::Answer,
                            None,
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
        .then(|| label(t::chat::pick_any(), TypeRole::Meta, Ink::TextMuted, a));
    let submit = QuestionDraft::needs_submit(q).then(|| submit_row(view, row, q, gate, a, cx));
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(header(
            Icon::Question,
            t::chat::claude_asks(),
            Ink::Working,
            step,
            a,
        ))
        .children(tabs)
        .child(label(&item.question, TypeRole::Heading, Ink::Text, a))
        .children(hint)
        .children(failed(view, a, cx))
        .children(options)
        .child(answer_box(view, row, q, gate, a, window, cx))
        .children(submit)
        .children(status(view, gate, a))
}

fn tabs(view: &ChatView, q: &PendingQuestion, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    div()
        .h(px(24.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .children(
            view.prompt
                .draft
                .tabs(q)
                .into_iter()
                .enumerate()
                .map(|(i, t)| {
                    let ink = if t.selected {
                        Ink::Text
                    } else if t.answered {
                        Ink::Done
                    } else {
                        Ink::TextMuted
                    };
                    div()
                        .id(SharedString::from(format!("prompt-tab-{i}")))
                        .h(px(24.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .rounded_full()
                        .cursor_pointer()
                        .when(t.selected, |p| p.fill(Ink::Raised, a))
                        .when(t.answered, |p| p.child(icon(Icon::Check, 10.0, ink, a)))
                        .child(label(&t.title, TypeRole::Label, ink, a).elide())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.prompt.draft.select_tab(i);
                            cx.notify();
                        }))
                }),
        )
}

fn answer_box(
    view: &ChatView,
    row: &str,
    q: &PendingQuestion,
    gate: Control,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let draft = &view.prompt.draft;
    let tab = draft.tab;
    let typed = draft.typed(tab).to_owned();
    let shown = match draft.text_answer(tab) {
        Some(answer) if typed.is_empty() => answer.to_owned(),
        _ => typed,
    };
    let focused = view.prompt.focus.is_focused(window);
    let (session, question) = (row.to_owned(), q.clone());
    let element = div()
        .id(SharedString::from(format!("prompt-answer-{tab}")))
        .child(field(&shown, t::chat::type_something_else(), focused, a).w_full())
        .when(!gate.enabled, |b| b.opacity(0.4));
    gate::tip(element, gate, a).when(gate.enabled, |b| {
        b.track_focus(&view.prompt.focus)
            .cursor_text()
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.prompt.focus);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let k = &event.keystroke;
                let modified = k.modifiers.platform || k.modifiers.control;
                let Some(typed) = this.prompt.draft.typed_mut() else {
                    return;
                };
                match type_key(typed, &k.key, k.key_char.as_deref(), modified) {
                    Typed::Submit => {
                        if let Some(answers) = this.prompt.draft.submit_text(&question) {
                            let request = Request::answer_for(&session, &question, &answers);
                            this.answer_prompt(
                                question_key(&question),
                                request,
                                Sending::Answer,
                                None,
                                cx,
                            );
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

fn submit_row(
    view: &ChatView,
    row: &str,
    q: &PendingQuestion,
    gate: Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let draft = &view.prompt.draft;
    let ready = Control {
        enabled: gate.enabled && draft.complete() && view.prompt.sending.is_none(),
        tooltip: gate.tooltip,
    };
    let multi = q.items.get(draft.tab).is_some_and(|i| i.multi_select);
    let next = (multi && q.items.len() > 1).then(|| {
        button(
            "prompt-next",
            t::chat::next(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.prompt.draft.next();
            cx.notify();
        }))
    });
    let (session, question) = (row.to_owned(), q.clone());
    let submit = button(
        "prompt-submit",
        t::chat::submit(),
        ButtonKind::Primary,
        gate::state(ready),
        a,
    );
    let submit = gate::tip(submit, ready, a).when(ready.enabled, |b| {
        b.on_click(cx.listener(move |this, _, _, cx| {
            if let Some(answers) = this.prompt.draft.answers(&question) {
                let request = Request::answer_for(&session, &question, &answers);
                this.answer_prompt(question_key(&question), request, Sending::Answer, None, cx);
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

fn permission(
    view: &ChatView,
    r: &HookRequest,
    gate: Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let mut choices = vec![(
        Decision::Allow,
        t::chat::allow(),
        t::chat::allow_this_time(),
    )];
    if r.can_always_allow {
        choices.push((
            Decision::Always,
            t::chat::allow_always(),
            t::chat::allow_always_note(),
        ));
    }
    choices.push((Decision::Deny, t::chat::deny(), t::chat::deny_note()));
    let on_screen = r.id.starts_with(NEEDS_PREFIX);
    let sending = view.prompt.sending;
    let chosen = match sending {
        Some(Sending::Decision { decision, .. }) => Some(decision),
        _ => None,
    };
    let options = choices
        .into_iter()
        .enumerate()
        .map(|(i, (decision, title, detail))| {
            let picked = chosen == Some(decision);
            let held = match sending {
                None => !gate.enabled,
                Some(_) => !picked,
            };
            let row = MenuItem::new(format!("prompt-permission-{i}"), title)
                .detail(detail)
                .inline()
                .checked(picked)
                .held(held)
                .build(a);
            let (key, request) = (r.id.clone(), permission_request(r, decision));
            let (sent, said) = (
                Sending::Decision {
                    decision,
                    on_screen,
                },
                Told::new(&r.tool_name, &r.summary, decision),
            );
            gate::tip(row, gate, a).when(gate.enabled && sending.is_none(), |el| {
                el.on_click(cx.listener(move |this, _, _, cx| {
                    this.answer_prompt(key.clone(), request.clone(), sent, Some(said.clone()), cx);
                }))
            })
        })
        .collect::<Vec<_>>();
    let file = file_name(&r.tool_name, &r.summary).map(|name| {
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(icon(
                Icon::File,
                TypeRole::Heading.spec().size,
                Ink::AccentText,
                a,
            ))
            .child(label(name, TypeRole::Heading, Ink::Text, a))
    });
    let open = view.prompt.command_open.as_deref() == Some(r.id.as_str());
    let shape = permission_text::block(&r.summary, open);
    let toggle = shape.max_lines.map(|_| {
        let key = r.id.clone();
        let words = if open {
            t::chat::show_less()
        } else {
            t::chat::show_all()
        };
        label(words, TypeRole::Meta, Ink::TextMuted, a)
            .id("prompt-command-toggle")
            .cursor_pointer()
            .hover(move |s| s.text_color(hsla(Ink::Text.color(a))))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.prompt.command_open = match this.prompt.command_open {
                    Some(_) => None,
                    None => Some(key.clone()),
                };
                cx.notify();
            }))
    });
    let description = permission_text::description_shown(&r.description, &r.summary);
    div()
        .flex()
        .flex_col()
        .gap(px(MENU_GAP))
        .child(header(
            Icon::Permission,
            t::chat::permission_needed(),
            Ink::AccentText,
            None,
            a,
        ))
        .child(label(
            &permission_text::headline(&r.tool_name, description),
            TypeRole::Heading,
            Ink::Text,
            a,
        ))
        .children(file)
        .child(code_block(&r.summary, shape.max_lines, shape.fade, a))
        .children(toggle.map(|t| div().flex().justify_end().child(t)))
        .children(failed(view, a, cx))
        .child(div().flex().flex_col().children(options))
        .children(status(view, gate, a))
}
