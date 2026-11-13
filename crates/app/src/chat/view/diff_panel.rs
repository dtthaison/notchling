use gpui::{div, prelude::*, px, AnyElement, SharedString, WeakEntity};
use notchling_kit::ui::{label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::page_panel::Shown;
use super::ChatView;
use crate::chat::git::DiffText;
use crate::chat::page_panel::Opens;

const PAD: f32 = 16.0;
const CHUNK_LINES: usize = 80;

pub enum DiffBody {
    Loading,
    Ready(DiffText),
    Failed(String),
}

pub struct DiffState {
    pub path: String,
    pub body: DiffBody,
}

pub fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn subtitle(path: &str, diff: Option<&DiffText>) -> String {
    let folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
    let what = match diff {
        Some(d) if d.untracked => "Untracked",
        _ => "Changes",
    };
    if folder.is_empty() {
        what.to_owned()
    } else {
        format!("{folder} \u{b7} {what}")
    }
}

pub fn shown(path: &str) -> Shown {
    Shown {
        opens: Opens::Diff(path.to_owned()),
        title: name_of(path).to_owned(),
        subtitle: subtitle(path, None),
        link: String::new(),
        error: None,
        file: None,
        diff: Some(DiffState {
            path: path.to_owned(),
            body: DiffBody::Loading,
        }),
    }
}

pub fn body(state: &DiffState, chat: WeakEntity<ChatView>, a: Appearance) -> AnyElement {
    let key = format!("diff-panel-{}", state.path);
    let note = |text: &str, ink: Ink| label(text, TypeRole::Meta, ink, a).into_any_element();
    let mut cut = false;
    let content: AnyElement = match &state.body {
        DiffBody::Loading => note("Reading the changes\u{2026}", Ink::TextMuted),
        DiffBody::Failed(why) => note(why, Ink::Expired),
        DiffBody::Ready(diff) if diff.runs.is_empty() => {
            note("There are no changes to show.", Ink::TextMuted)
        }
        DiffBody::Ready(diff) => {
            cut = diff.truncated;
            let mut chunks: Vec<(Ink, String)> = Vec::new();
            for (kind, text) in &diff.runs {
                let lines: Vec<&str> = text.split('\n').collect();
                for chunk in lines.chunks(CHUNK_LINES) {
                    chunks.push((kind.ink(), chunk.join("\n")));
                }
            }
            div()
                .w_full()
                .kit_type(TypeRole::Mono)
                .flex()
                .flex_col()
                .children(chunks.into_iter().enumerate().map(|(i, (ink, text))| {
                    div().ink(ink, a).child(super::markdown::plain(
                        &format!("{key}-r{i}"),
                        &super::markdown::Sel::File,
                        &text,
                        a,
                    ))
                }))
                .into_any_element()
        }
    };
    div()
        .id(SharedString::from(format!("{key}-scroll")))
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .p(px(PAD))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(super::select::listener(chat))
                .child(content)
                .when(cut, |d| {
                    d.child(label(
                        "Only the first part of this diff is shown.",
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    ))
                }),
        )
        .into_any_element()
}
