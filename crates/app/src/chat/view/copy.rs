//! Copy in Chat's window: ⌘C, the Edit menu's Copy and the right-click menu's (`copy_menu`) all
//! come here, as the `Copy` action.
//!
//! The action is handled on the window's root, which GPUI reaches only through the focused
//! element's ancestors. With nothing focused (the reply box is off for a session the app can't
//! type into, or was never clicked) the path is the window's outermost node alone, so a handler on
//! the root saw neither the key nor the action and a selection could not be copied. [`Keys`] is an
//! anchor: a focus handle on a zero-size element that takes the keyboard whenever nothing else
//! has it (a click in the reply box or a terminal still takes it away), so the root is always on
//! the path. It is a separate handle from the root's own, since an element that tracks focus takes
//! it on every click inside it, which would pull the keyboard out of the reply box each time a
//! message was clicked.

use gpui::{div, prelude::*, ClipboardItem, Context, Div, FocusHandle, Window};

use super::{row_select, select, ChatView};

/// The anchor that keeps the window's own keys (Copy) reachable.
pub struct Keys {
    focus: FocusHandle,
}

impl Keys {
    pub fn new(cx: &mut Context<ChatView>) -> Keys {
        Keys {
            focus: cx.focus_handle(),
        }
    }

    /// Takes the keyboard when nothing has it. Called as the window draws.
    pub fn hold(&self, window: &mut Window, cx: &gpui::App) {
        if window.focused(cx).is_none() {
            self.focus.focus(window);
        }
    }

    /// The zero-size element that carries the handle, a child of the window's root.
    pub fn anchor(&self) -> Div {
        div().absolute().size_0().track_focus(&self.focus)
    }
}

/// What Copy takes, in the order a person means it: the conversation's selection, else the file
/// panel's, else the reply box's. Each later one is asked only if the earlier ones hold nothing.
pub fn text_to_copy(
    conversation: Option<String>,
    file: impl FnOnce() -> Option<String>,
    reply: impl FnOnce() -> Option<String>,
) -> Option<String> {
    row_select::clipboard_text(conversation, file).or_else(reply)
}

impl ChatView {
    /// What is selected in the conversation or the file panel (not the reply box): what the
    /// right-click menu copies.
    pub(super) fn shown_selection(&self, cx: &gpui::App) -> Option<String> {
        let conversation = self.parts.conversation.read(cx).copied(cx);
        text_to_copy(conversation, select::copied, || None)
    }

    /// What Copy would take now: see [`text_to_copy`]. The reply box counts only while it has the
    /// keyboard, so a selection left in it doesn't answer for another place.
    pub(super) fn selection_text(&self, window: &Window, cx: &gpui::App) -> Option<String> {
        let conversation = self.parts.conversation.read(cx).copied(cx);
        text_to_copy(conversation, select::copied, || {
            let editor = self.tools.editor.read(cx);
            editor
                .is_focused(window)
                .then(|| editor.selected_text())
                .flatten()
        })
    }

    /// Puts the selection on the clipboard; whether there was one.
    pub(super) fn copy_selection(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        let Some(text) = self.selection_text(window, cx) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(text: &str) -> Option<String> {
        Some(text.to_owned())
    }

    #[test]
    fn the_conversation_comes_first_then_the_file_then_the_reply_box() {
        let nothing = || -> Option<String> { unreachable!("not asked") };
        assert_eq!(
            text_to_copy(some("chat"), nothing, nothing).as_deref(),
            Some("chat")
        );
        assert_eq!(
            text_to_copy(None, || some("file"), nothing).as_deref(),
            Some("file")
        );
        assert_eq!(
            text_to_copy(None, || None, || some("reply")).as_deref(),
            Some("reply")
        );
        assert_eq!(text_to_copy(None, || None, || None), None);
    }
}
