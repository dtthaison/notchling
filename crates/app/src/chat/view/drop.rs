use notchling_locale as t;
use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, px, Context, Div, ExternalPaths, Window};
use notchling_kit::metric::RADIUS_CARD;
use notchling_kit::ui::{card, hsla, label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::ChatView;
use crate::chat::drops;

pub const PREVIEW_ENV: &str = "NOTCHLING_RENDER_DROP";
pub const GROUP: &str = "chat-column";
const INSET: f32 = 8.0;
const EDGE: f32 = 1.5;

pub fn hint(preview: bool, a: Appearance) -> Div {
    let sentence = card(a)
        .px(px(14.0))
        .py(px(8.0))
        .border_color(hsla(Ink::Accent.color(a).with_alpha(0.6)))
        .child(label(t::chat::drop_hint(), TypeRole::Heading, Ink::Text, a));
    div()
        .absolute()
        .inset_0()
        .p(px(INSET))
        .opacity(if preview { 1.0 } else { 0.0 })
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
    pub(super) fn attached(&self, row: &str) -> Vec<PathBuf> {
        drops::attachments_from(self.tools.draft(&drops::attachments_key(row)))
    }

    pub(super) fn set_attached(&mut self, row: &str, files: &[PathBuf], cx: &mut Context<Self>) {
        let key = drops::attachments_key(row);
        self.put_draft(&key, drops::attachments_text(files), cx);
        cx.notify();
    }

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

    pub(super) fn drop_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        assert_eq!(t::chat::drop_hint(), "Drop to add to your reply");
        assert!(t::chat::drop_hint().lines().count() == 1);
    }
}
