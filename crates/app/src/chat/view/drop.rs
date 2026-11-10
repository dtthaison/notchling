//! Files dropped on the middle column (header, conversation, reply box), as the Swift
//! `SessionDetail` takes them on its conversation column: they are attached to the reply (a tile
//! each over the box, a picture shown as itself, × takes one off), and their paths follow the
//! words when it is sent (`chat::drops`; a screenshot thumbnail's short-lived file is copied first,
//! under the data folder's `drops/`). The attached files are kept with the drafts, so they survive
//! a quit. The caret comes back to the box. While a drag is over the column a
//! hint covers it (a wash with a dashed accent edge and one sentence), drawn with the kit's inks
//! and never taking the mouse; the composer's own edge turns accent as well. Nothing is offered
//! while the composer is off (acting off, a read-only session): no target, no hint.

use std::path::{Path, PathBuf};

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
    /// The files attached to `row`'s reply, in the order they came.
    pub(super) fn attached(&self, row: &str) -> Vec<PathBuf> {
        drops::attachments_from(self.tools.draft(&drops::attachments_key(row)))
    }

    /// Sets `row`'s attached files (kept with the drafts).
    pub(super) fn set_attached(&mut self, row: &str, files: &[PathBuf], cx: &mut Context<Self>) {
        let key = drops::attachments_key(row);
        self.put_draft(&key, drops::attachments_text(files), cx);
        cx.notify();
    }

    /// Takes one file off the selected row's reply (its tile's ×).
    pub(super) fn remove_attached(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(row) = self.selected.clone() else {
            return;
        };
        let files: Vec<PathBuf> = self
            .attached(&row)
            .into_iter()
            .filter(|f| f != path)
            .collect();
        self.set_attached(&row, &files, cx);
    }

    /// Attaches dropped files to the selected row's reply and gives the box the focus back.
    pub(super) fn drop_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The screenshot thumbnail's file is gone soon after the drop: the box gets a copy that stays.
        let paths = drops::keep(
            paths,
            &self.root.data_dir.join("drops"),
            crate::phone::bridge::now_ms(),
        );
        if let Some(row) = self.selected.clone() {
            let mut files = self.attached(&row);
            for path in paths {
                if !files.contains(&path) {
                    files.push(path);
                }
            }
            self.set_attached(&row, &files, cx);
        }
        self.tools
            .editor
            .update(cx, |editor, _| editor.focus(window));
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
