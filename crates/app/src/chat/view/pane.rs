//! The conversation's scrolling column as a view of its own, drawn cached: GPUI keeps its layout
//! and paint from the last frame until it is told to draw again, so a frame that only moves a
//! spinner or the live line's clock in the rest of the window costs that part alone. Before, every
//! such frame laid out every message (a long conversation: 50 ms a frame, a core).
//!
//! It draws again whenever `ChatView` notifies (it observes the view, whose state it draws), when
//! it is scrolled or hovered (GPUI notifies the view an element is in), and on a window refresh.
//! The spinners and the clock wake only themselves or the window's outer parts, never `ChatView`'s
//! observers.

use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{div, prelude::*, AnyView, Context, StyleRefinement, Subscription, WeakEntity, Window};
use notchling_kit::ui::{scrollable, window_appearance};

use super::{conversation, now_ms, ChatView};

/// Times the conversation was drawn, for `--render`'s bench.
pub static RENDERS: AtomicU64 = AtomicU64::new(0);

pub struct ConversationPane {
    chat: WeakEntity<ChatView>,
    _changes: Subscription,
}

impl ConversationPane {
    pub fn new(chat: &gpui::Entity<ChatView>, cx: &mut Context<Self>) -> Self {
        ConversationPane {
            chat: chat.downgrade(),
            _changes: cx.observe(chat, |_, _, cx| cx.notify()),
        }
    }
}

/// An empty view in `ChatView`'s tree, outside the pane: waking it draws the window's other parts
/// again (GPUI redraws a view's ancestors) without telling the pane, for a change only they show
/// (the live line's clock, the Phone button's dot).
pub struct Chrome;

impl Render for Chrome {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().absolute().size_0()
    }
}

impl ChatView {
    /// Draws the window again but not the conversation, whose cached drawing stays: for a change
    /// shown only outside it. `cx.notify()` draws the conversation too.
    pub(super) fn redraw_chrome(&self, cx: &mut Context<ChatView>) {
        self.chrome.update(cx, |_, cx| cx.notify());
    }
}

/// The pane as an element of `ChatView`'s tree: it fills the column's room above the reply box.
pub fn element(pane: &gpui::Entity<ConversationPane>) -> AnyView {
    AnyView::from(pane.clone()).cached(StyleRefinement::default().flex_1().min_h_0().w_full())
}

impl Render for ConversationPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        RENDERS.fetch_add(1, Ordering::Relaxed);
        let a = window_appearance(window);
        let Some(chat) = self.chat.upgrade() else {
            return div().into_any_element();
        };
        chat.update(cx, |chat, cx| {
            // A hovered message's relative time is kept fresh from when it was last drawn.
            chat.drawn_ms = now_ms();
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    scrollable(conversation::render(chat, a, cx))
                        .tracking(&chat.conversation_scroll),
                )
                .into_any_element()
        })
    }
}
