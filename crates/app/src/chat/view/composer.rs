//! The reply box, after the Swift app's `Composer`: one rounded card (fill card, radius composer,
//! hairline that turns accent while focused, soft shadow) holding the text and the send disc, with
//! nothing drawn inside it but the text itself. The text grows from one line to [`MAX_LINES`],
//! then scrolls. Return sends, Shift or Option Return adds a line. Under the card, one row: the
//! model or options picker at the left, the status line (the error, "Sending…", what waits, or why
//! the box is off) and Stop while the turn runs. Above it, an Antigravity permission card when the
//! daemon holds one for the row, and the picker's pop-up. Disabled while acting isn't on; the
//! reason is the status line, and only there.

use gpui::{div, point, prelude::*, px, relative, BoxShadow, Context, Div, Stateful, Window};
use notchling_kit::metric::{RADIUS_COMPOSER, SEND_DISC, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::typescale::sentence;
use notchling_kit::ui::{button, button_with, hsla, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::tools::Picker;
use super::ChatView;
use crate::chat::data::Tool;
use crate::mac::gate;
use crate::notch::answer::{type_key, Typed};
use crate::tools::agy_options;
use crate::tools::requests::is_draft;
use crate::tools::view::{agy_card, agy_options_card, codex_picker_card, PickerAction};

/// The text grows to this many lines, then scrolls (the Swift `ReplyMeasurer.maxLines`).
pub const MAX_LINES: usize = 8;
/// One line of body text in the box, in points: the system font at 13 lays out at 16.
pub const LINE_HEIGHT: f32 = 16.0;
/// The card's edge: 1 pt hairline at rest, 1.5 pt accent while focused (the spec's Composer).
const EDGE_REST: f32 = 1.0;
const EDGE_FOCUSED: f32 = 1.5;
/// Inside the card: 14 before the text, 8 after the disc, 8 above and below (the Swift paddings),
/// plus the text's own 6 above and below so one line centres on the 30 pt disc.
const PAD_LEADING: f32 = 14.0;
const PAD_TRAILING: f32 = 8.0;
const PAD_VERTICAL: f32 = 8.0;
const TEXT_PAD_VERTICAL: f32 = 6.0;

/// The text area's height for `lines` lines of text: one at least, [`MAX_LINES`] at most.
pub fn text_height(lines: usize) -> f32 {
    LINE_HEIGHT * lines.clamp(1, MAX_LINES) as f32
}

/// What Return does in the box: with Shift or Option it adds a line, bare it sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnKey {
    Newline,
    Send,
}

pub fn return_key(key: &str, shift: bool, alt: bool) -> Option<ReturnKey> {
    (key == "enter").then_some(if shift || alt {
        ReturnKey::Newline
    } else {
        ReturnKey::Send
    })
}

/// The card's padding at an edge width, so focusing (a thicker edge) moves nothing inside.
fn inset(base: f32, edge: f32) -> f32 {
    base - (edge - EDGE_REST)
}

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

/// The pop-up over the composer, when open: its bottom edge on the composer's top.
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
            .bottom(relative(1.0))
            .left(px(SIDE_PADDING))
            .child(card),
    )
}

/// The Antigravity permission the daemon holds for the row, above the box.
fn permission(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let held = view.agy_prompt()?;
    let gate = view.acting(cx).control();
    let permission = held.permission.clone();
    let card = agy_card(&permission, None, gate, a, cx, move |this, option, cx| {
        this.answer_agy(&held, option.to_owned(), cx);
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

/// The status line under the card: the error, "Sending…", what waits, or why the box is off.
/// Shown once, here, as a sentence; the box itself carries no tooltip for it.
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
    } else {
        (control.tooltip?.to_owned(), Ink::TextFaint)
    };
    Some(label(&sentence(&text), TypeRole::Meta, ink, a).line_clamp(2))
}

/// The picker button under the card, for a Codex or Antigravity session.
fn picker(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<impl IntoElement> {
    let title = picker_label(view)?;
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
    // Meta type in the muted ink with a chevron, as the Swift pickers under its composer.
    let ink = if has_options {
        Ink::TextMuted
    } else {
        Ink::TextFaint
    };
    let title = label(&format!("{title} ▾"), TypeRole::Meta, ink, a);
    let b = button_with("picker", title, kind, state, a)
        .h(px(20.0))
        .px(px(6.0));
    let b = gate::tip(
        b,
        crate::acting::Control {
            enabled: true,
            tooltip: tip,
        },
        a,
    );
    Some(b.when(has_options, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.toggle_picker(cx)))
    }))
}

/// The send disc: 30 pt, accent with the arrow in onAccent; raised and faint while there is
/// nothing to send, when it also takes no click (the Swift button is disabled then).
fn send_disc(
    enabled: bool,
    empty: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let live = enabled && !empty;
    let (fill, glyph) = if live {
        (Ink::Accent, Ink::OnAccent)
    } else {
        (Ink::Raised, Ink::TextFaint)
    };
    div()
        .id("send")
        .size(px(SEND_DISC))
        .flex_shrink_0()
        .rounded_full()
        .fill(fill, a)
        .flex()
        .items_center()
        .justify_center()
        .kit_type(TypeRole::Heading)
        .ink(glyph, a)
        .child(Icon::Send.glyph())
        .when(live, |d| {
            d.cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.send_text(cx)))
        })
}

pub fn render(view: &ChatView, a: Appearance, window: &Window, cx: &mut Context<ChatView>) -> Div {
    let control = view.composer_control(cx);
    let id = view.selected.clone().unwrap_or_default();
    let text = view.tools.draft(&id).to_owned();
    let empty = text.trim().is_empty();
    let focused = control.enabled && view.tools.focus.is_focused(window);
    let edge = if focused { EDGE_FOCUSED } else { EDGE_REST };
    let edge_color = if focused {
        Ink::Accent.color(a).with_alpha(0.6)
    } else {
        Ink::Hairline.color(a)
    };
    // The text itself, wrapping at the card's width: one line to MAX_LINES, then it scrolls.
    let text_area = div()
        .id("composer-text")
        .flex_1()
        .min_w(px(0.0))
        .py(px(TEXT_PAD_VERTICAL))
        .min_h(px(text_height(1) + 2.0 * TEXT_PAD_VERTICAL))
        .max_h(px(text_height(MAX_LINES) + 2.0 * TEXT_PAD_VERTICAL))
        .overflow_y_scroll()
        .track_scroll(&view.tools.scroll)
        .kit_type(TypeRole::Body)
        .line_height(px(LINE_HEIGHT))
        .ink(if empty { Ink::TextFaint } else { Ink::Text }, a)
        .when(!control.enabled, |t| t.opacity(0.5))
        .child(if text.is_empty() {
            placeholder(view).to_owned()
        } else {
            text.clone()
        });
    let card = div()
        .id("composer")
        .mx(px(SIDE_PADDING))
        .pl(px(inset(PAD_LEADING, edge)))
        .pr(px(inset(PAD_TRAILING, edge)))
        .py(px(inset(PAD_VERTICAL, edge)))
        .flex()
        .items_end()
        .gap(px(10.0))
        .rounded(px(RADIUS_COMPOSER))
        .fill(Ink::Card, a)
        .border(px(edge))
        .border_color(hsla(edge_color))
        .shadow(vec![BoxShadow {
            color: hsla(Ink::Shadow.color(a)),
            offset: point(px(0.0), px(6.0)),
            blur_radius: px(14.0),
            spread_radius: px(0.0),
        }])
        .child(text_area)
        .child(send_disc(control.enabled, empty, a, cx))
        .when(control.enabled, |c| {
            c.track_focus(&view.tools.focus)
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
                    match return_key(&k.key, k.modifiers.shift, k.modifiers.alt) {
                        Some(ReturnKey::Newline) if !modified => draft.push('\n'),
                        Some(ReturnKey::Send) if !modified => this.send_text(cx),
                        _ => match type_key(draft, &k.key, k.key_char.as_deref(), modified) {
                            Typed::Submit => this.send_text(cx),
                            Typed::Changed => {}
                            Typed::Ignored => return,
                        },
                    }
                    // Past the last line the box scrolls, and what was just typed stays in view.
                    this.tools.scroll.scroll_to_bottom();
                    cx.stop_propagation();
                    cx.notify();
                }))
        });
    let stop = view.selected_working().then(|| {
        let gate = view.acting(cx).control();
        let b = button("stop", "Stop", ButtonKind::Ghost, gate::state(gate), a);
        gate::tip(b, gate, a).when(gate.enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        })
    });
    let footer = div()
        .mx(px(SIDE_PADDING))
        .pt(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .children(picker(view, a, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .children(status(view, control, a)),
        )
        .children(stop);
    div()
        .relative()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .pt(px(8.0))
        .pb(px(SIDE_PADDING))
        .children(permission(view, a, cx))
        .child(card)
        .child(footer)
        .children(pop_up(view, a, cx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_grows_one_line_to_eight_then_stops() {
        assert_eq!(text_height(0), LINE_HEIGHT);
        assert_eq!(text_height(1), LINE_HEIGHT);
        assert_eq!(text_height(3), 3.0 * LINE_HEIGHT);
        assert_eq!(text_height(8), 8.0 * LINE_HEIGHT);
        assert_eq!(text_height(9), 8.0 * LINE_HEIGHT);
        assert_eq!(text_height(100), text_height(MAX_LINES));
    }

    #[test]
    fn return_sends_and_shift_or_option_return_adds_a_line() {
        assert_eq!(return_key("enter", false, false), Some(ReturnKey::Send));
        assert_eq!(return_key("enter", true, false), Some(ReturnKey::Newline));
        assert_eq!(return_key("enter", false, true), Some(ReturnKey::Newline));
        assert_eq!(return_key("enter", true, true), Some(ReturnKey::Newline));
        assert_eq!(return_key("a", true, false), None);
        assert_eq!(return_key("backspace", false, false), None);
    }

    #[test]
    fn a_thicker_focused_edge_moves_nothing_inside() {
        assert_eq!(inset(PAD_LEADING, EDGE_REST) + EDGE_REST, PAD_LEADING + 1.0);
        assert_eq!(
            inset(PAD_LEADING, EDGE_FOCUSED) + EDGE_FOCUSED,
            PAD_LEADING + 1.0
        );
        assert_eq!(
            inset(PAD_VERTICAL, EDGE_FOCUSED) + EDGE_FOCUSED,
            inset(PAD_VERTICAL, EDGE_REST) + EDGE_REST
        );
    }
}
