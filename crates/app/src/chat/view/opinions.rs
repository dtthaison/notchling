//! Second opinions in the conversation (`SecondOpinionButton`, `SecondOpinionCards`): a small
//! button on a reply or an edit opens the sheet, and the opinions asked under that message show as
//! cards: running with Cancel, done with the reply, failed with the error. The store is the
//! engine's, shared app-wide (`AppOpinions`); a render and a fixture root have none, so the
//! button is off.

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_engine::delegate::second_opinion::{SecondOpinion, Status, Subject};
use notchling_engine::delegate::second_opinion_store::SecondOpinionStore;
use notchling_engine::transcript::model::{ChatItem, ItemKind, ToolKind};
use notchling_kit::labels::Corner;
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::ChatView;
use crate::acting::Control;
use crate::mac::gate;
use crate::mac::shell::AppOpinions;
use crate::tools::second_opinion::tool_name;

/// The button's hover.
pub const TIP: &str = "Second Opinion: ask Codex or Antigravity to review this";
/// Why the button is off.
pub const NO_STORE: &str = "Second opinions need the app's own data folder, not a fixture";

/// The store, when this app has one.
pub fn store(cx: &Context<ChatView>) -> Option<SecondOpinionStore> {
    cx.try_global::<AppOpinions>().and_then(|o| o.0.clone())
}

/// What a conversation item can have reviewed, each with the anchor its card sits under: a
/// reply (the item.s id), or an edit.s diff (the call.s id).
pub fn subjects(item: &ChatItem) -> Vec<(String, Subject)> {
    match &item.kind {
        ItemKind::Assistant { text } if !text.trim().is_empty() => {
            vec![(item.id.clone(), Subject::Message(text.clone()))]
        }
        ItemKind::Tools(calls) => calls
            .iter()
            .filter(|c| matches!(c.kind, ToolKind::Edit | ToolKind::Write) && !c.diff.is_empty())
            .map(|c| {
                let path = c
                    .input
                    .lines()
                    .find(|l| l.starts_with('/'))
                    .unwrap_or(&c.title)
                    .to_owned();
                (
                    c.id.clone(),
                    Subject::Change {
                        path,
                        diff: c.diff.clone(),
                    },
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The small button that opens the sheet for `subject` under `anchor`.
pub fn opinion_button(
    anchor: &str,
    subject: Subject,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> impl IntoElement {
    let control = match store(cx) {
        Some(_) => Control {
            enabled: true,
            tooltip: Some(TIP),
        },
        None => Control {
            enabled: false,
            tooltip: Some(NO_STORE),
        },
    };
    let anchor = anchor.to_owned();
    let b = button(
        SharedString::from(format!("opinion-{anchor}")),
        "Second Opinion",
        ButtonKind::Ghost,
        gate::state(control),
        a,
    );
    gate::tip(b, control, a)
        .when(control.enabled, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| {
                this.open_second_opinion(subject.clone(), &anchor, cx)
            }))
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

/// One opinion's card.
fn card_for(run: &SecondOpinion, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let tool = tool_name(run.tool_value());
    let title = match run.status {
        Status::Running => format!("{tool} is reviewing {}", run.subject),
        Status::Done => format!("{tool} on {}", run.subject),
        Status::Failed => format!("{tool} couldn't review {}", run.subject),
        Status::Cancelled => format!("{tool} on {}, cancelled", run.subject),
    };
    let pick = pick_line(run);
    let id = run.id.clone();
    let action = if run.status == Status::Running {
        button(
            SharedString::from(format!("opinion-cancel-{}", run.id)),
            "Cancel",
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
        button(
            SharedString::from(format!("opinion-remove-{}", run.id)),
            "×",
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
                        "Cancelled before it answered.",
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

/// The cards for the opinions asked under `anchors` in `session`.
pub fn cards(
    session: &str,
    anchors: &[String],
    a: Appearance,
    cx: &mut Context<ChatView>,
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

/// Whether an opinion in the selected session is still running (the view redraws for its clock).
pub fn any_running(view: &ChatView, cx: &Context<ChatView>) -> bool {
    let Some(session) = &view.selected else {
        return false;
    };
    store(cx).is_some_and(|s| {
        s.opinions()
            .iter()
            .any(|o| o.session == *session && o.status == Status::Running)
    })
}
