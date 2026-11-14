use gpui::{div, prelude::*, px, App, Context, Div, SharedString, WeakEntity};
use notchling_engine::delegate::second_opinion::{SecondOpinion, Status, Subject};
use notchling_engine::delegate::second_opinion_store::SecondOpinionStore;
use notchling_engine::transcript::model::{ChatItem, ItemKind, ToolKind};
use notchling_kit::labels::Corner;
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, icon_button, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::conversation_view::ConversationView;
use super::ChatView;
use crate::acting::Control;
use crate::mac::gate;
use crate::mac::shell::AppOpinions;
use crate::tools::second_opinion::tool_name;

pub fn store(cx: &App) -> Option<SecondOpinionStore> {
    cx.try_global::<AppOpinions>().and_then(|o| o.0.clone())
}

pub fn subjects(item: &ChatItem) -> Vec<(String, Subject)> {
    match &item.kind {
        ItemKind::Assistant { text } if !text.trim().is_empty() => {
            vec![(item.id.clone(), Subject::Message(text.clone()))]
        }
        ItemKind::Tools(calls) => {
            let mut out: Vec<(String, Subject)> = Vec::new();
            let edits = calls.iter().filter(|c| {
                matches!(c.kind, ToolKind::Edit | ToolKind::Write) && !c.diff.is_empty()
            });
            for c in edits {
                let path = c
                    .input
                    .lines()
                    .find(|l| l.starts_with('/'))
                    .unwrap_or(&c.title)
                    .to_owned();
                let same = out.iter_mut().find_map(|(_, s)| match s {
                    Subject::Change { path: p, diff } if *p == path => Some(diff),
                    _ => None,
                });
                match same {
                    Some(diff) => diff.extend(c.diff.iter().cloned()),
                    None => out.push((
                        c.id.clone(),
                        Subject::Change {
                            path,
                            diff: c.diff.clone(),
                        },
                    )),
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

pub fn button_label(subject: &Subject, of: usize) -> String {
    match subject {
        Subject::Change { path, .. } if of > 1 => {
            let name = path.rsplit('/').next().unwrap_or(path);
            t::chat::second_opinion_of(name)
        }
        _ => t::chat::second_opinion().to_owned(),
    }
}

pub fn opinion_button(
    anchor: &str,
    subject: Subject,
    of: usize,
    a: Appearance,
    chat: &WeakEntity<ChatView>,
    cx: &mut Context<ConversationView>,
) -> impl IntoElement {
    let control = match store(cx) {
        Some(_) => Control {
            enabled: true,
            tooltip: Some(t::chat::second_opinion_tip()),
        },
        None => Control {
            enabled: false,
            tooltip: Some(t::chat::no_store()),
        },
    };
    let anchor = anchor.to_owned();
    let chat = chat.clone();
    let title = button_label(&subject, of);
    let b = button(
        SharedString::from(format!("opinion-{anchor}")),
        &title,
        ButtonKind::Ghost,
        gate::state(control),
        a,
    );
    gate::tip(b, control, a)
        .when(control.enabled, |b| {
            b.on_click(move |_, _, cx| {
                let (subject, anchor) = (subject.clone(), anchor.clone());
                chat.update(cx, |this, cx| {
                    this.open_second_opinion(subject, &anchor, cx)
                })
                .ok();
            })
        })
        .part_at("Second Opinion Button", Corner::TopTrailing, a, cx)
}

fn pick_line(run: &SecondOpinion) -> String {
    [run.model.as_deref(), run.effort.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn card_for(run: &SecondOpinion, a: Appearance, cx: &mut Context<ConversationView>) -> Div {
    let tool = tool_name(run.tool_value());
    let title = match run.status {
        Status::Running => t::chat::opinion_running(tool, &run.subject),
        Status::Done => t::chat::opinion_done(tool, &run.subject),
        Status::Failed => t::chat::opinion_failed(tool, &run.subject),
        Status::Cancelled => t::chat::opinion_cancelled(tool, &run.subject),
    };
    let pick = pick_line(run);
    let id = run.id.clone();
    let action = if run.status == Status::Running {
        button(
            SharedString::from(format!("opinion-cancel-{}", run.id)),
            t::chat::cancel(),
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |_, _, _, cx| {
            if let Some(store) = store(cx) {
                store.cancel(&id);
            }
            cx.notify();
        }))
    } else {
        icon_button(
            SharedString::from(format!("opinion-remove-{}", run.id)),
            Icon::Close,
            10.0,
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |_, _, _, cx| {
            if let Some(store) = store(cx) {
                store.remove(&id);
            }
            cx.notify();
        }))
    };
    let header = div()
        .min_h(px(34.0))
        .pl(px(12.0))
        .pr(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(label(&title, TypeRole::Label, Ink::Text, a).elide())
        .when(!pick.is_empty(), |h| {
            h.child(label(&pick, TypeRole::Meta, Ink::TextFaint, a))
        })
        .when(run.seconds > 0.0, |h| {
            h.child(label(
                &crate::chat::format::duration(run.seconds),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
        })
        .child(div().flex_1())
        .child(action);
    let body = (run.status != Status::Running).then(|| {
        div()
            .px(px(14.0))
            .py(px(12.0))
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .children(
                run.error
                    .as_deref()
                    .filter(|_| run.status == Status::Failed)
                    .map(|e| label(e, TypeRole::Body, Ink::Expired, a)),
            )
            .when(!run.reply.is_empty(), |b| {
                b.child(label(&run.reply, TypeRole::Body, Ink::Text, a))
            })
            .when(
                run.reply.is_empty() && run.status == Status::Cancelled,
                |b| {
                    b.child(label(
                        t::chat::opinion_cancelled_early(),
                        TypeRole::Body,
                        Ink::TextMuted,
                        a,
                    ))
                },
            )
    });
    card(a)
        .rounded(px(RADIUS_MD))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(header)
        .children(body)
        .part_inside("Second Opinion Card", Corner::TopTrailing, a, cx)
}

pub fn cards(
    session: &str,
    anchors: &[String],
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Option<Div> {
    let runs = store(cx)?.runs(session, anchors);
    if runs.is_empty() {
        return None;
    }
    Some(
        div()
            .pt(px(6.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(runs.iter().map(|run| card_for(run, a, cx))),
    )
}

pub fn any_running(view: &ChatView, cx: &App) -> bool {
    let Some(session) = &view.selected else {
        return false;
    };
    store(cx).is_some_and(|s| {
        s.opinions()
            .iter()
            .any(|o| o.session == *session && o.status == Status::Running)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::{DiffKind, DiffLine, ToolCall};

    fn edit(id: &str, path: &str, line: &str) -> ToolCall {
        let mut call = ToolCall::new(
            id,
            "Edit",
            ToolKind::Edit,
            format!("Edited {path}"),
            path.into(),
        );
        call.diff = vec![DiffLine {
            kind: DiffKind::Add,
            text: line.into(),
            number: Some(1),
        }];
        call
    }

    fn group(calls: Vec<ToolCall>) -> ChatItem {
        ChatItem {
            id: "g".into(),
            kind: ItemKind::Tools(calls),
            date: None,
        }
    }

    #[test]
    fn two_edits_of_one_file_are_one_review_with_both_changes() {
        let item = group(vec![
            edit("c1", "/w/a.rs", "one"),
            edit("c2", "/w/a.rs", "two"),
        ]);
        let found = subjects(&item);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "c1", "under the first edit");
        match &found[0].1 {
            Subject::Change { path, diff } => {
                assert_eq!(path, "/w/a.rs");
                assert_eq!(diff.iter().filter(|l| l.kind == DiffKind::Add).count(), 2);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(button_label(&found[0].1, found.len()), "Second Opinion");
    }

    #[test]
    fn edits_of_two_files_name_their_file() {
        let item = group(vec![
            edit("c1", "/w/a.rs", "one"),
            edit("c2", "/w/b.rs", "two"),
        ]);
        let found = subjects(&item);
        assert_eq!(found.len(), 2);
        assert_eq!(
            button_label(&found[1].1, found.len()),
            "Second Opinion: b.rs"
        );
    }
}
