//! The row under a message, shown while the pointer is over the message or the row. Under yours,
//! at the right: the delivered mark, how long ago, "·", Copy. Under Claude's reply, at the left:
//! Copy, then how long ago. The time comes from the transcript line's timestamp
//! (`chat::message_meta`), with the full local date as its tooltip. Copy puts the message's text
//! (a reply's raw Markdown) on the clipboard and shows a check for a moment.
//!
//! The row is always laid out, so showing it moves nothing, and is drawn clear until the message's
//! group is hovered (the group is the message and the row together, so the pointer can go from one
//! to the other). Rewind, Fork, Pin and Read Aloud are not here: see `chat::message_meta`.

use gpui::{
    div, prelude::*, px, svg, AnyElement, ClipboardItem, Context, Div, SharedString, Stateful,
};
use notchling_engine::transcript::model::ChatItem;
use notchling_kit::icon::svg_box;
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::ui::{hsla, icon, icon_in, label, parts_on, KitPart};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::conversation_view::{ConversationState, ConversationView};
use super::now_ms;
use crate::chat::message_meta::{self, Plan, Role};
use crate::chat::rows::Row;
use crate::mac::gate;

/// The row's height, the buttons' size plus a hair.
pub(super) const ROW_HEIGHT: f32 = 24.0;
/// A button's box.
const BUTTON: f32 = 22.0;
/// The icons' size (points, as `icon` takes them).
const ICON: f32 = 14.0;

/// A faint icon button: its icon is `TextFaint` and turns `Text` under the pointer, which has the
/// pointing hand. An SVG takes its color from its own style, so the turn is a group hover on the
/// icon itself.
fn ghost_icon(id: SharedString, glyph: Icon, a: Appearance) -> Stateful<Div> {
    let group = SharedString::from(format!("{id}-hot"));
    let faint = Ink::TextFaint.color(a);
    let hot = Ink::Text.color(a);
    let drawn = match glyph.svg() {
        Some(path) => {
            let size = px(svg_box(ICON));
            div().child(
                svg()
                    .path(path)
                    .size(size)
                    .text_color(hsla(faint))
                    .group_hover(group.clone(), move |s| s.text_color(hsla(hot))),
            )
        }
        None => div().child(icon_in(glyph, ICON, faint)),
    };
    div()
        .id(id)
        .group(group)
        .flex_shrink_0()
        .size(px(BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .child(drawn)
}

/// The Copy button: the copy icon, or a check for a moment after a click.
fn copy_button(
    id: &str,
    text: &str,
    copied: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Stateful<Div> {
    let text = text.to_owned();
    let key = id.to_owned();
    let glyph = if copied { Icon::Check } else { Icon::Copy };
    let button = ghost_icon(SharedString::from(format!("copy-msg-{id}")), glyph, a).on_click(
        cx.listener(move |this, _, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            let at = now_ms();
            this.s.copied = Some((key.clone(), at));
            cx.notify();
        }),
    );
    gate::tip_text(button, if copied { "Copied" } else { "Copy" }.into(), a)
}

/// "4 minutes ago", its tooltip the full date and time.
fn time_label(id: &str, at: f64, a: Appearance, cx: &Context<ConversationView>) -> Stateful<Div> {
    let text = message_meta::relative_local(now_ms() as f64 / 1000.0, at);
    let tip = message_meta::full_local(at);
    let time = label(&text, TypeRole::Meta, Ink::TextFaint, a)
        .id(SharedString::from(format!("time-{id}")))
        .flex_shrink_0()
        .px(px(4.0));
    gate::tip_text(time, tip.into(), a).part("Message Time", a, cx)
}

/// The double check on your message.
fn delivered_mark(id: &str, a: Appearance) -> Stateful<Div> {
    let mark = div()
        .id(SharedString::from(format!("delivered-{id}")))
        .flex_shrink_0()
        .child(icon(Icon::Delivered, ICON, Ink::TextFaint, a));
    gate::tip_text(mark, "Delivered".into(), a)
}

fn separator(a: Appearance) -> Div {
    label("·", TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0()
}

/// The row for a message of `role` with its `plan`.
#[allow(clippy::too_many_arguments)]
fn actions_row(
    role: Role,
    plan: Plan,
    id: &str,
    text: &str,
    date: Option<f64>,
    copied: bool,
    shown: bool,
    group: SharedString,
    trailing: Option<Div>,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Div {
    let time = date.filter(|_| plan.time);
    let row = div()
        .w_full()
        .min_h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(2.0))
        .when(!shown, |d| {
            d.opacity(0.0).group_hover(group, |s| s.opacity(1.0))
        });
    match role {
        Role::User => {
            let lead = plan.delivered || time.is_some();
            row.justify_end()
                .pr(px(2.0))
                .when(plan.delivered, |d| d.child(delivered_mark(id, a)))
                .children(time.map(|at| time_label(id, at, a, cx)))
                .when(lead && plan.copy, |d| d.child(separator(a)))
                .when(plan.copy, |d| d.child(copy_button(id, text, copied, a, cx)))
        }
        Role::Assistant => row
            .justify_start()
            .when(plan.copy, |d| d.child(copy_button(id, text, copied, a, cx)))
            .children(time.map(|at| time_label(id, at, a, cx)))
            // Second Opinion, at the far right.
            .when_some(trailing, |d, t| {
                d.child(div().flex_1().min_w(px(0.0)).flex().justify_end().child(t))
            }),
    }
    .part("Message Actions", a, cx)
}

/// `element` (a message) with its row beneath, when it is a message that has one. `item` is the
/// transcript line it came from; a queued bubble has none and no row.
pub fn with_actions(
    s: &ConversationState,
    row: &Row,
    item: Option<&ChatItem>,
    element: Div,
    trailing: Option<Div>,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> AnyElement {
    let (role, text, queued) = match row {
        Row::User { text, queued, .. } => (Role::User, text, *queued),
        Row::Assistant { text } => (Role::Assistant, text, false),
        _ => return hover_only(s, item, element, trailing, cx),
    };
    let Some(item) = item else {
        return element.into_any_element();
    };
    let date = item.date;
    let plan = message_meta::plan(role, queued, date.is_some(), text);
    // Only a reply's row carries Second Opinion; anywhere else it gets a row of its own.
    if !plan.any() || (role == Role::User && trailing.is_some()) {
        return hover_only(s, Some(item), element, trailing, cx);
    }
    let id = item.id.as_str();
    let group = SharedString::from(format!("msg-{id}"));
    let shown = s.actions_preview || parts_on(cx);
    let hover_id = id.to_owned();
    let items_end = role == Role::User;
    let copied = s
        .copied
        .as_ref()
        .is_some_and(|(key, at)| key == id && message_meta::copied_shows(*at, now_ms()));
    let row = actions_row(
        role,
        plan,
        id,
        text,
        date,
        copied,
        shown,
        group.clone(),
        trailing,
        a,
        cx,
    );
    div()
        .id(group.clone())
        .group(group)
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .when(items_end, |d| d.items_end())
        .child(element)
        .child(row)
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            this.hover(&hover_id, *hovered, cx);
        }))
        .into_any_element()
}

/// `element` with `trailing` (the Second Opinion buttons) on a row of its own beneath, shown only
/// while the pointer is over the two, for a row that has no actions of its own to share.
fn hover_only(
    s: &ConversationState,
    item: Option<&ChatItem>,
    element: Div,
    trailing: Option<Div>,
    cx: &mut Context<ConversationView>,
) -> AnyElement {
    let Some(trailing) = trailing else {
        return element.into_any_element();
    };
    let key = item.map(|i| i.id.as_str()).unwrap_or("row");
    let group = SharedString::from(format!("opinion-{key}"));
    let shown = s.actions_preview || parts_on(cx);
    div()
        .id(group.clone())
        .group(group.clone())
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(element)
        .child(
            div()
                .w_full()
                .min_h(px(ROW_HEIGHT))
                .flex()
                .items_center()
                .justify_end()
                .when(!shown, |d| {
                    d.opacity(0.0).group_hover(group, |s| s.opacity(1.0))
                })
                .child(trailing),
        )
        .into_any_element()
}
