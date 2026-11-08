//! The Accounts window's parts over GPUI (macOS): the shared account row (avatar, email, subtitle,
//! In Use or Switch, the ⋯ menu, a content slot), limit bars and the Chat header's meters, the add
//! panel in its phases, a segmented control and the alert over the window. Each takes the pure
//! model and the owning view's handlers; nothing here reads a store.

use std::rc::Rc;

use gpui::{div, prelude::*, px, Context, Div, ElementId, SharedString, Stateful};
use notchling_kit::metric::{AVATAR, METER_BAR, RADIUS_MD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{avatar, button, card, hsla, label, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::flows::{AddPanel, Alert};
use super::limits::{Fill, LimitGroup, LimitLine, Meter};
use super::model::{AccountRow, RowContent};
use crate::mac::gate;

/// A handler for one of a part's events.
pub type Handler<V, E> = Rc<dyn Fn(&mut V, E, &mut Context<V>)>;

/// The ink a bar is filled with: green under 70, orange to 90, red from 90.
pub fn fill_ink(fill: Fill) -> Ink {
    match fill {
        Fill::Fine => Ink::Done,
        Fill::Warm => Ink::Needs,
        Fill::Hot => Ink::Expired,
    }
}

/// A capsule bar filled to `percent`.
fn bar(percent: f64, fill: Fill, height: f32, a: Appearance) -> Div {
    div()
        .flex_1()
        .h(px(height))
        .rounded_full()
        .fill(Ink::Raised, a)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(percent))
                .rounded_full()
                .fill(fill_ink(fill), a),
        )
}

fn relative(percent: f64) -> gpui::DefiniteLength {
    gpui::relative((percent / 100.0).clamp(0.0, 1.0) as f32)
}

/// The Swift `LimitRow`: label 52 wide, the bar, the percent right-aligned, when it resets.
pub fn limit_line(line: &LimitLine, a: Appearance) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            label(&line.label, TypeRole::Meta, Ink::TextMuted, a)
                .w(px(56.0))
                .truncate(),
        )
        .child(bar(line.percent, line.fill, 6.0, a))
        .child(
            label(&line.percent_text, TypeRole::Label, Ink::Text, a)
                .w(px(36.0))
                .text_right(),
        )
        .child(
            label(
                line.resets.as_deref().unwrap_or(""),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            )
            .w(px(118.0))
            .text_right()
            .truncate(),
        )
}

/// The Swift `GroupedLimits`: each group's name over its bars.
pub fn limit_groups(groups: &[LimitGroup], a: Appearance) -> Div {
    let mut column = div().flex().flex_col().gap(px(10.0)).w_full();
    for group in groups {
        let mut block = div().flex().flex_col().gap(px(6.0));
        if let Some(name) = &group.name {
            block = block.child(label(name, TypeRole::Meta, Ink::TextMuted, a));
        }
        for line in &group.lines {
            block = block.child(limit_line(line, a));
        }
        column = column.child(block);
    }
    column
}

/// A Chat header meter (`SessionLimit`): label and percent over a thin bar, the reset under it.
pub fn meter(id: impl Into<ElementId>, m: &Meter, a: Appearance) -> Stateful<Div> {
    let tip: SharedString = m.tooltip.clone().into();
    let element = div()
        .id(id)
        .w(px(64.0))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .flex()
                .justify_between()
                .child(label(&m.label, TypeRole::Meta, Ink::TextMuted, a))
                .child(label(&m.percent_text, TypeRole::Meta, Ink::Text, a)),
        )
        .child(bar(m.percent, m.fill, METER_BAR, a))
        .children(
            m.resets
                .as_deref()
                .map(|r| label(r, TypeRole::Meta, Ink::TextFaint, a).truncate()),
        );
    gate::tip_text(element, tip, a)
}

/// What a row's controls do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowEvent {
    Switch,
    ToggleMenu,
    Remove,
}

/// The shared account row (`AccountRow`). `menu_open` draws the ⋯ menu's items under it.
pub fn account_row<V: 'static>(
    row: &AccountRow,
    menu_open: bool,
    a: Appearance,
    cx: &mut Context<V>,
    on: Handler<V, RowEvent>,
) -> Div {
    let key = format!("{}-{}", row.provider, row.id);
    let mut right = div().flex().items_center().gap(px(8.0)).flex_shrink_0();
    if row.in_use {
        right = right.child(
            div()
                .h(px(20.0))
                .px(px(8.0))
                .rounded_full()
                .fill(Ink::AccentWash, a)
                .flex()
                .items_center()
                .child(label("In Use", TypeRole::Label, Ink::AccentText, a)),
        );
    }
    if let Some(control) = row.switch {
        let handler = on.clone();
        let b = button(
            SharedString::from(format!("switch-{key}")),
            "Switch",
            ButtonKind::Secondary,
            gate::state(control),
            a,
        );
        right = right.child(gate::tip(b, control, a).when(control.enabled, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| handler(this, RowEvent::Switch, cx)))
        }));
    }
    if !row.menu.is_empty() {
        let handler = on.clone();
        right = right.child(
            button(
                SharedString::from(format!("menu-{key}")),
                Icon::More.glyph(),
                if menu_open {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Ghost
                },
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| handler(this, RowEvent::ToggleMenu, cx))),
        );
    }
    let content = match &row.content {
        RowContent::None => None,
        RowContent::Text { text, warning } => Some(
            label(
                text,
                TypeRole::Meta,
                if *warning { Ink::Needs } else { Ink::TextMuted },
                a,
            )
            .into_any_element(),
        ),
        RowContent::Limits(groups) => Some(limit_groups(groups, a).into_any_element()),
    };
    let menu = menu_open.then(|| {
        let mut items = card(a)
            .absolute()
            .right(px(10.0))
            .top(px(44.0))
            .p(px(4.0))
            .shadow_lg();
        for (i, item) in row.menu.iter().enumerate() {
            let handler = on.clone();
            items = items.child(
                div()
                    .id(SharedString::from(format!("menu-item-{key}-{i}")))
                    .px(px(10.0))
                    .py(px(5.0))
                    .rounded(px(RADIUS_SM))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
                    .child(label(item.title, TypeRole::Label, Ink::Text, a))
                    .on_click(
                        cx.listener(move |this, _, _, cx| handler(this, RowEvent::Remove, cx)),
                    ),
            );
        }
        items
    });
    card(a)
        .relative()
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .when(row.in_use, |c| c.border_color(hsla(Ink::Accent.color(a))))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(avatar(&row.email, AVATAR, None, Ink::Card, a))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(label(&row.email, TypeRole::Heading, Ink::Text, a).truncate())
                        .when(!row.subtitle.is_empty(), |c| {
                            c.child(label(&row.subtitle, TypeRole::Meta, Ink::TextMuted, a))
                        }),
                )
                .child(right),
        )
        .children(content)
        .children(
            row.cost
                .as_deref()
                .map(|c| label(c, TypeRole::Meta, Ink::TextFaint, a)),
        )
        .children(menu)
}

/// What the add panel's controls do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddEvent {
    Add,
    /// Open Again / Open Page.
    Open,
    CopyLink,
    Cancel,
    /// Done after Added, or Cancel after Failed: back to idle.
    Dismiss,
}

/// The add panel (`SignInPanel` and the providers' panels), under the list.
pub fn add_panel<V: 'static>(
    panel: &AddPanel,
    a: Appearance,
    cx: &mut Context<V>,
    on: Handler<V, AddEvent>,
) -> Div {
    let act = |id: &str, title: &str, kind: ButtonKind, event: AddEvent, cx: &mut Context<V>| {
        let handler = on.clone();
        button(
            SharedString::from(format!("add-{id}")),
            title,
            kind,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| handler(this, event.clone(), cx)))
    };
    let base = div()
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .border_t(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)));
    match panel {
        AddPanel::Idle { note, add } => {
            let handler = on.clone();
            let b = button(
                "add-account",
                "Add Account",
                ButtonKind::Primary,
                gate::state(*add),
                a,
            );
            let b = gate::tip(b, *add, a).when(add.enabled, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| handler(this, AddEvent::Add, cx)))
            });
            base.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(label(note, TypeRole::Meta, Ink::TextMuted, a).flex_1())
                    .child(b),
            )
        }
        AddPanel::Waiting {
            line,
            note,
            url,
            copy_link,
        } => {
            let mut buttons = div().flex().gap(px(8.0));
            if url.is_some() {
                let title = if *copy_link {
                    "Open Again"
                } else {
                    "Open Page"
                };
                buttons = buttons.child(act(
                    "open",
                    title,
                    ButtonKind::Secondary,
                    AddEvent::Open,
                    cx,
                ));
                if *copy_link {
                    buttons = buttons.child(act(
                        "copy",
                        "Copy Link",
                        ButtonKind::Secondary,
                        AddEvent::CopyLink,
                        cx,
                    ));
                }
            }
            buttons = buttons.child(act(
                "cancel",
                "Cancel",
                ButtonKind::Ghost,
                AddEvent::Cancel,
                cx,
            ));
            base.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(label(
                        Icon::Working.glyph(),
                        TypeRole::Heading,
                        Ink::Working,
                        a,
                    ))
                    .child(label(line, TypeRole::Body, Ink::Text, a)),
            )
            .child(label(note, TypeRole::Meta, Ink::TextMuted, a))
            .child(buttons)
        }
        AddPanel::Saving => base.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(label(
                    Icon::Working.glyph(),
                    TypeRole::Heading,
                    Ink::Working,
                    a,
                ))
                .child(label("Saving the account…", TypeRole::Body, Ink::Text, a)),
        ),
        AddPanel::Added { line } => base.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(label(Icon::Done.glyph(), TypeRole::Heading, Ink::Done, a))
                .child(label(line, TypeRole::Body, Ink::Text, a).flex_1())
                .child(act(
                    "another",
                    "Add Another",
                    ButtonKind::Secondary,
                    AddEvent::Add,
                    cx,
                ))
                .child(act(
                    "done",
                    "Done",
                    ButtonKind::Ghost,
                    AddEvent::Dismiss,
                    cx,
                )),
        ),
        AddPanel::Failed { message } => base.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(label(Icon::Needs.glyph(), TypeRole::Heading, Ink::Needs, a))
                .child(
                    label(message, TypeRole::Body, Ink::Text, a)
                        .flex_1()
                        .line_clamp(2),
                )
                .child(act(
                    "retry",
                    "Try Again",
                    ButtonKind::Primary,
                    AddEvent::Add,
                    cx,
                ))
                .child(act(
                    "dismiss",
                    "Cancel",
                    ButtonKind::Ghost,
                    AddEvent::Dismiss,
                    cx,
                )),
        ),
    }
}

/// A segmented control: one button per option, the picked one filled.
pub fn segmented<V: 'static>(
    id: &str,
    options: &[(String, String)],
    picked: &str,
    a: Appearance,
    cx: &mut Context<V>,
    on: Handler<V, String>,
) -> Div {
    let mut row = div()
        .flex()
        .p(px(2.0))
        .gap(px(2.0))
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, a);
    for (key, title) in options {
        let handler = on.clone();
        let key_out = key.clone();
        let kind = if key == picked {
            ButtonKind::Secondary
        } else {
            ButtonKind::Ghost
        };
        row = row.child(
            button(
                SharedString::from(format!("{id}-{key}")),
                title,
                kind,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| handler(this, key_out.clone(), cx))),
        );
    }
    row
}

/// The alert over a dimmed view: title, message, Cancel and its button.
pub fn alert_overlay<V: 'static>(
    alert: &Alert,
    busy: bool,
    a: Appearance,
    cx: &mut Context<V>,
    on: Handler<V, bool>,
) -> Div {
    let (yes, no) = (on.clone(), on);
    let state = if busy {
        PartState::Disabled
    } else {
        PartState::Rest
    };
    let confirm = button(
        "alert-confirm",
        &alert.button,
        if alert.destructive {
            ButtonKind::Secondary
        } else {
            ButtonKind::Primary
        },
        state,
        a,
    )
    .when(!busy, |b| {
        b.on_click(cx.listener(move |this, _, _, cx| yes(this, true, cx)))
    });
    let cancel = button("alert-cancel", "Cancel", ButtonKind::Ghost, state, a).when(!busy, |b| {
        b.on_click(cx.listener(move |this, _, _, cx| no(this, false, cx)))
    });
    div()
        .absolute()
        .size_full()
        // The backdrop takes every click and hover: no row under the alert is live.
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(Ink::Shadow.color(a)))
        .child(
            card(a)
                .w(px(400.0))
                .p(px(20.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(label(&alert.title, TypeRole::Heading, Ink::Text, a))
                .child(label(&alert.message, TypeRole::Body, Ink::TextMuted, a))
                .child(
                    div()
                        .pt(px(6.0))
                        .flex()
                        .justify_end()
                        .gap(px(8.0))
                        .child(cancel)
                        .child(confirm),
                ),
        )
}

/// A faint line of mono text (a folder, a command).
pub fn mono(text: &str, a: Appearance) -> Div {
    div()
        .font_family(MONO_FONT)
        .text_size(px(12.0))
        .ink(Ink::Text, a)
        .rounded(px(RADIUS_MD))
        .child(text.to_owned())
}
