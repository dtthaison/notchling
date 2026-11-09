//! Files dropped on the middle column (header, conversation, reply box), as the Swift
//! `SessionDetail` takes them on its conversation column: the paths go into the reply box at the
//! caret (`chat::drops`) and the caret comes back to the box. While a drag is over the column a
//! hint covers it (a wash with a dashed accent edge and one sentence), drawn with the kit's inks
//! and never taking the mouse; the composer's own edge turns accent as well. Nothing is offered
//! while the composer is off (acting off, a read-only session): no target, no hint.

use std::path::PathBuf;

use gpui::{div, prelude::*, px, Context, Div, ExternalPaths, Window};
use notchling_kit::metric::RADIUS_CARD;
use notchling_kit::ui::{card, hsla, label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::ChatView;
use crate::chat::drops;

/// Set to `1`, a render draws the hint as a drag over the column would (nothing drags in a
/// picture). Read once, by `ChatView::new`.
pub const PREVIEW_ENV: &str = "NOTCHLING_RENDER_DROP";
/// The column's group, which the hint watches for a drag.
pub const GROUP: &str = "chat-column";
/// The sentence on the hint.
pub const HINT: &str = "Drop to add to your reply";
/// The hint's inset from the column's edges, and its edge.
const INSET: f32 = 8.0;
const EDGE: f32 = 1.5;

/// The hint over the column: there at every render at opacity 0, shown at 1 while a file drag is
/// over the column's group, or when `preview` asks for it.
pub fn hint(preview: bool, a: Appearance) -> Div {
    let sentence = card(a)
        .px(px(14.0))
        .py(px(8.0))
        .border_color(hsla(Ink::Accent.color(a).with_alpha(0.6)))
        .child(label(HINT, TypeRole::Heading, Ink::Text, a));
    div()
        .absolute()
        .inset_0()
        .p(px(INSET))
        .opacity(if preview { 1.0 } else { 0.0 })
        // A group style is only computed on an element with a hitbox; naming a group of its own
        // gives it one that blocks nothing under it.
        .group("chat-drop-hint")
        .group_drag_over::<ExternalPaths>(GROUP, |style| style.opacity(1.0))
        .child(
            div()
                .size_full()
                .rounded(px(RADIUS_CARD))
                .fill(Ink::AccentWash, a)
                .border(px(EDGE))
                .border_dashed()
                .border_color(hsla(Ink::Accent.color(a)))
                .flex()
                .items_center()
                .justify_center()
                .child(sentence),
        )
}

impl ChatView {
    /// Puts dropped paths into the reply box at the caret and gives it the focus back.
    pub(super) fn drop_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tools.editor.update(cx, |editor, cx| {
            let mut edit = editor.edit().clone();
            if drops::insert_paths(&mut edit, paths) {
                editor.set_edit(edit, cx);
            }
            editor.focus(window);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_is_one_plain_sentence() {
        assert_eq!(HINT, "Drop to add to your reply");
        assert!(HINT.lines().count() == 1);
    }
}
