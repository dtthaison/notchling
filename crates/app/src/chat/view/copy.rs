use gpui::{div, prelude::*, ClipboardItem, Context, Div, FocusHandle, Window};

use super::{row_select, select, ChatView};

pub struct Keys {
    focus: FocusHandle,
}

impl Keys {
    pub fn new(cx: &mut Context<ChatView>) -> Keys {
        Keys {
            focus: cx.focus_handle(),
        }
    }

    pub fn hold(&self, window: &mut Window, cx: &gpui::App) {
        if window.focused(cx).is_none() {
            self.focus.focus(window);
        }
    }

    pub fn anchor(&self) -> Div {
        div().absolute().size_0().track_focus(&self.focus)
    }
}

pub fn text_to_copy(
    conversation: Option<String>,
    file: impl FnOnce() -> Option<String>,
    reply: impl FnOnce() -> Option<String>,
) -> Option<String> {
    row_select::clipboard_text(conversation, file).or_else(reply)
}

impl ChatView {
    pub(super) fn shown_selection(&self, cx: &gpui::App) -> Option<String> {
        let conversation = self.parts.conversation.read(cx).copied(cx);
        text_to_copy(conversation, select::copied, || None)
    }

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
