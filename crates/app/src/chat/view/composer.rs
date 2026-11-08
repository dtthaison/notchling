//! The reply box (`ReplyEditor`, one line here): what is typed goes to the row's tool on Return,
//! with Stop while its turn runs and the model or options picker at the left. Above it, an
//! Antigravity permission card when the daemon holds one for the row, and the picker's pop-up.
//! Disabled, with the reason on hover, while acting isn't on.

use gpui::{div, prelude::*, px, Context, Div, Window};
use notchling_kit::metric::{RADIUS_COMPOSER, SEND_DISC, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, field, hsla, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::tools::Picker;
use super::ChatView;
use crate::chat::data::Tool;
use crate::mac::gate;
use crate::notch::answer::{type_key, Typed};
use crate::tools::agy_options;
use crate::tools::requests::is_draft;
use crate::tools::view::{agy_card, agy_options_card, codex_picker_card, PickerAction};

fn placeholder(view: &ChatView) -> &'static str {
    match view.selected_session() {
        Some(s) if is_draft(&s.agent.id) => "Your first message starts the session",
        Some(s) if s.tool == Tool::Codex => "Message Codex…",
        Some(s) if s.tool == Tool::Antigravity => "Message Antigravity…",
        _ => "Reply…",
    }
}

/// The picker button's label: the Codex pick, or Antigravity's current options.
fn picker_label(view: &ChatView) -> Option<String> {
    let session = view.selected_session()?;
    let id = &session.agent.id;
    match session.tool {
        Tool::Codex => Some(match &view.tools.picker {
            Some(Picker::Codex(p)) => p.label(),
            _ => {
                let pick = view.tools.picks.get(id).cloned().unwrap_or_default();
                crate::tools::codex_picker::CodexPicker::new(
                    Vec::new(),
                    pick,
                    &view.conversation.facts,
                )
                .label()
            }
        }),
        Tool::Antigravity => Some(agy_options::label(
            &view.tools.agy_config,
            !is_draft(id),
            view.tools.agy.as_ref().is_some_and(|a| a.loading),
        )),
        Tool::Claude => None,
    }
}

/// The pop-up over the box, when open.
fn pop_up(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let gate = view.acting(cx).control();
    let card = match &view.tools.picker {
        Some(Picker::Codex(picker)) => codex_picker_card(picker, a, cx, |this, action, cx| {
            if let Some(Picker::Codex(picker)) = &mut this.tools.picker {
                match action {
                    PickerAction::Model(slug) => picker.choose_model(&slug),
                    PickerAction::Effort(slug, effort) => picker.choose_effort(&slug, &effort),
                    PickerAction::Reset => picker.reset(),
                }
                let picked = picker.clone();
                this.keep_pick(&picked);
            }
            cx.notify();
        }),
        Some(Picker::Agy) => agy_options_card(
            &view.tools.agy_config,
            gate,
            a,
            cx,
            |this, option, value, cx| this.set_agy_option(option, value, cx),
        ),
        None => return None,
    };
    Some(
        div()
            .absolute()
            .bottom(px(72.0))
            .left(px(SIDE_PADDING))
            .child(card),
    )
}

/// The Antigravity permission the daemon holds for the row, above the box.
fn permission(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let (prompt_id, agy) = view.agy_prompt()?;
    let gate = view.acting(cx).control();
    let rpc = agy.rpc_id.clone();
    let card = agy_card(&agy, None, gate, a, cx, move |this, option, cx| {
        this.answer_agy(prompt_id.clone(), rpc.clone(), option.to_owned(), cx);
    });
    Some(
        div()
            .mx(px(SIDE_PADDING))
            .mb(px(8.0))
            .p(px(14.0))
            .rounded(px(RADIUS_COMPOSER))
            .fill(Ink::Card, a)
            .border(px(1.0))
            .border_color(hsla(Ink::Accent.color(a).with_alpha(0.45)))
            .child(card),
    )
}

/// The line under the box: the error, "Sending…", what waits, or why the box is off.
fn status(view: &ChatView, control: crate::acting::Control, a: Appearance) -> Option<Div> {
    let (text, ink) = if let Some(error) = &view.tools.error {
        (error.clone(), Ink::Needs)
    } else if view.tools.sending {
        ("Sending…".to_owned(), Ink::TextMuted)
    } else if let Some(n) = view
        .selected
        .as_deref()
        .map(|id| view.tools.live.waiting(id))
        .filter(|n| *n > 0)
    {
        (
            if n == 1 {
                "1 message waits for this turn to end.".to_owned()
            } else {
                format!("{n} messages wait for this turn to end.")
            },
            Ink::TextMuted,
        )
    } else if let Some(why) = control.tooltip {
        (why.to_owned(), Ink::TextFaint)
    } else {
        return None;
    };
    Some(
        label(&text, TypeRole::Meta, ink, a)
            .px(px(SIDE_PADDING))
            .line_clamp(2),
    )
}

pub fn render(view: &ChatView, a: Appearance, window: &Window, cx: &mut Context<ChatView>) -> Div {
    let control = view.composer_control(cx);
    let id = view.selected.clone().unwrap_or_default();
    let text = view.tools.draft(&id).to_owned();
    let focused = view.tools.focus.is_focused(window);
    let box_element = div()
        .id("composer-field")
        .flex_1()
        .min_w(px(0.0))
        .child(field(&text, placeholder(view), focused, a).w_full())
        .when(!control.enabled, |b| b.opacity(0.5));
    let box_element = gate::tip(box_element, control, a).when(control.enabled, |b| {
        b.track_focus(&view.tools.focus)
            .cursor_text()
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.tools.focus);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                let k = &event.keystroke;
                let modified = k.modifiers.platform || k.modifiers.control;
                let Some(id) = this.selected.clone() else {
                    return;
                };
                let draft = this.tools.drafts.entry(id).or_default();
                match type_key(draft, &k.key, k.key_char.as_deref(), modified) {
                    Typed::Submit => this.send_text(cx),
                    Typed::Changed => {}
                    Typed::Ignored => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
    });
    let picker = picker_label(view).map(|title| {
        let has_options = matches!(
            view.selected_session().map(|s| s.tool),
            Some(Tool::Codex) | Some(Tool::Antigravity)
        ) && !(view.tools.agy_config.is_empty()
            && view
                .selected_session()
                .is_some_and(|s| s.tool == Tool::Antigravity));
        let state = if has_options {
            PartState::Rest
        } else {
            PartState::Disabled
        };
        let kind = if view.tools.picker.is_some() {
            ButtonKind::Secondary
        } else {
            ButtonKind::Ghost
        };
        let tip = view
            .selected_session()
            .filter(|s| s.tool == Tool::Antigravity)
            .map(|s| agy_options::tip(!is_draft(&s.agent.id)));
        let b = button("picker", &title, kind, state, a);
        let b = gate::tip(
            b,
            crate::acting::Control {
                enabled: true,
                tooltip: tip,
            },
            a,
        );
        b.when(has_options, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.toggle_picker(cx)))
        })
    });
    let stop = view.selected_working().then(|| {
        let gate = view.acting(cx).control();
        let b = button("stop", "Stop", ButtonKind::Secondary, gate::state(gate), a);
        gate::tip(b, gate, a).when(gate.enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        })
    });
    let send = div()
        .id("send")
        .size(px(SEND_DISC))
        .flex_shrink_0()
        .rounded_full()
        .fill(Ink::Accent, a)
        .flex()
        .items_center()
        .justify_center()
        .ink(Ink::OnAccent, a)
        .child(Icon::Send.glyph())
        .when(!control.enabled || text.trim().is_empty(), |d| {
            d.opacity(0.45)
        })
        .when(control.enabled, |d| {
            d.cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.send_text(cx)))
        });
    div()
        .relative()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .pt(px(8.0))
        .pb(px(SIDE_PADDING))
        .children(permission(view, a, cx))
        .child(
            div()
                .mx(px(SIDE_PADDING))
                .min_h(px(44.0))
                .pl(px(8.0))
                .pr(px(8.0))
                .py(px(7.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .rounded(px(RADIUS_COMPOSER))
                .fill(Ink::Card, a)
                .border(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .children(picker)
                .child(box_element)
                .children(stop)
                .child(send),
        )
        .children(status(view, control, a).map(|s| s.pt(px(6.0))))
        .children(pop_up(view, a, cx))
}
