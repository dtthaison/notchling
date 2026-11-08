//! The Allow/Deny prompt over GPUI (part U6, macOS): the card for a phone that gave the right
//! pairing code. It is its own small floating window (`mac::shell` opens it the moment the daemon
//! asks and closes it when the answer is in), so it shows whether or not the Phone panel is open,
//! the way the Swift app's alert did. Esc denies; Return does not allow, so a key pressed in
//! another app a moment before the window appeared can't let a phone in. Every call to the daemon
//! runs on the background executor. A render has a fixed prompt and no daemon behind it.

use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyView, Context, FocusHandle, Focusable, Render, Window};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, window_appearance, KitStyled};
use notchling_kit::{Ink, TypeRole};

use super::approval::{Prompt, Queue};
use super::bridge::{now_ms, Bridge};
use super::pairing::PairingFeed;

pub struct PairPromptView {
    feed: Arc<PairingFeed>,
    bridge: Arc<Bridge>,
    /// A render's prompt; the feed's own otherwise.
    fixed: Option<Prompt>,
    /// An answer is on its way: the buttons wait for it.
    busy: bool,
    /// The last answer the daemon didn't take, until the next click.
    note: Option<String>,
    focus: FocusHandle,
}

impl PairPromptView {
    /// The live prompt over the feed's queue.
    pub fn live(feed: Arc<PairingFeed>, bridge: Arc<Bridge>, cx: &mut Context<Self>) -> Self {
        PairPromptView {
            feed,
            bridge,
            fixed: None,
            busy: false,
            note: None,
            focus: cx.focus_handle(),
        }
    }

    /// A render's: `prompt` as given, with nothing behind it.
    pub fn fixed(prompt: Prompt, cx: &mut Context<Self>) -> Self {
        PairPromptView {
            feed: Arc::new(PairingFeed::none()),
            bridge: Arc::new(Bridge::fixed(super::bridge::Snapshot::detached())),
            fixed: Some(prompt),
            busy: false,
            note: None,
            focus: cx.focus_handle(),
        }
    }

    fn shown(&self) -> Option<Prompt> {
        self.fixed.clone().or_else(|| self.feed.prompt(now_ms()))
    }

    /// Sends the person's answer, then reads the bridge again (an allowed phone is a new device).
    fn answer(&mut self, request: String, allow: bool, cx: &mut Context<Self>) {
        if self.busy || !self.feed.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        let (feed, bridge) = (self.feed.clone(), self.bridge.clone());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = feed.answer(&request, allow);
                    bridge.refresh(now_ms());
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.note = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

impl Focusable for PairPromptView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PairPromptView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        if !self.focus.is_focused(window) && self.fixed.is_none() {
            window.focus(&self.focus);
        }
        let Some(prompt) = self.shown() else {
            return div().size_full().fill(Ink::Window, a).into_any_element();
        };
        let state = if self.busy {
            PartState::Disabled
        } else {
            PartState::Rest
        };
        let (deny_id, allow_id) = (prompt.request.clone(), prompt.request.clone());
        let deny = button("pair-deny", prompt.deny, ButtonKind::Secondary, state, a).when(
            !self.busy,
            |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    this.answer(deny_id.clone(), false, cx);
                }))
            },
        );
        let allow = button("pair-allow", prompt.allow, ButtonKind::Primary, state, a).when(
            !self.busy,
            |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    this.answer(allow_id.clone(), true, cx);
                }))
            },
        );
        let esc_request = prompt.request.clone();
        div()
            .id("pair-prompt")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.answer(esc_request.clone(), false, cx);
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(SIDE_PADDING + 4.0))
            .rounded(px(RADIUS_CARD))
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .fill(Ink::Window, a)
            .child(div().flex().items_center().gap(px(8.0)).child(label(
                prompt.title,
                TypeRole::Title,
                Ink::Text,
                a,
            )))
            .child(label(&prompt.message, TypeRole::Body, Ink::Text, a))
            .children(
                prompt
                    .from
                    .as_deref()
                    .map(|from| label(from, TypeRole::Mono, Ink::TextMuted, a)),
            )
            .children(
                prompt
                    .more
                    .as_deref()
                    .map(|more| label(more, TypeRole::Meta, Ink::TextFaint, a)),
            )
            .children(
                self.note
                    .as_deref()
                    .map(|note| label(note, TypeRole::Meta, Ink::Needs, a)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(deny)
                    .child(allow),
            )
            .into_any_element()
    }
}

/// `--render phone-pair-request`: a made-up phone asking, a second one waiting behind it.
pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    if name != super::renders::PAIR_REQUEST {
        return None;
    }
    let mut queue = Queue::default();
    for request in super::renders::pair_requests() {
        queue.push(request, super::renders::NOW_MS);
    }
    let prompt = queue.prompt(super::renders::NOW_MS)?;
    Some(cx.new(|cx| PairPromptView::fixed(prompt, cx)).into())
}
