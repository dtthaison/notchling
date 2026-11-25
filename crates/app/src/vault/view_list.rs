use gpui::{div, prelude::*, px, AnyElement, Context, Div, SharedString};
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, label, scrollable, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::copy_name;
use super::form::Form;
use super::list::{Row, RENEW_TIP};
use super::view::VaultSheetView;
use super::view_reveal::{field_row, FieldLine};
use crate::mac::gate;

pub(super) fn list_row(
    row: &Row,
    copied: bool,
    lines: &[FieldLine],
    a: Appearance,
    cx: &mut Context<VaultSheetView>,
) -> Div {
    let name = row.name.clone();
    let mut tags = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(label(&row.name, TypeRole::Heading, Ink::Text, a))
        .child(copy_button(&name, copied, a, cx))
        .child(label(row.kind, TypeRole::Meta, Ink::TextFaint, a));
    if row.asks_first {
        tags = tags.child(label("Asks first", TypeRole::Meta, Ink::AccentText, a));
    }
    let mut details = div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(tags);
    if let Some(url) = &row.url {
        details = details.child(label(url, TypeRole::Meta, Ink::TextMuted, a).elide());
    }
    details = details.child(
        div()
            .font_family(MONO_FONT)
            .text_size(px(11.0))
            .ink(Ink::TextFaint, a)
            .child(row.vars.clone()),
    );
    for line in lines {
        details = details.child(field_row(&row.name, line, a, cx));
    }
    if let Some(note) = &row.note {
        details = details.child(label(note, TypeRole::Meta, Ink::TextMuted, a).line_clamp(2));
    }
    if let Some(expiry) = &row.expiry {
        details = details.child(label(
            &expiry.text,
            TypeRole::Meta,
            if expiry.expired {
                Ink::Expired
            } else {
                Ink::TextMuted
            },
            a,
        ));
    }
    let mut buttons = div().flex().items_center().gap(px(4.0)).flex_shrink_0();
    if row.renew.is_none() {
        buttons = buttons.child(
            button(
                SharedString::from(format!("renew-gap-{name}")),
                "Renew…",
                ButtonKind::Ghost,
                PartState::Disabled,
                a,
            )
            .invisible(),
        );
    }
    if let Some(control) = row.renew {
        let n = name.clone();
        let tip = if control.enabled {
            Some(RENEW_TIP)
        } else {
            control.tooltip
        };
        let c = crate::acting::Control {
            enabled: control.enabled,
            tooltip: tip,
        };
        let b = button(
            SharedString::from(format!("renew-{name}")),
            "Renew…",
            ButtonKind::Ghost,
            gate::state(c),
            a,
        );
        buttons = buttons.child(gate::tip(b, c, a).when(c.enabled, |b| {
            b.on_click(cx.listener(move |this, _, window, cx| this.renew(&n, window, cx)))
        }));
    }
    let n = name.clone();
    let edit = button(
        SharedString::from(format!("edit-{name}")),
        "Edit",
        ButtonKind::Ghost,
        gate::state(row.edit),
        a,
    );
    buttons = buttons.child(gate::tip(edit, row.edit, a).when(row.edit.enabled, |b| {
        b.on_click(cx.listener(move |this, _, window, cx| {
            if let Some(entry) = this.entry(&n) {
                this.open_form(Form::edit(&entry), window, cx);
            }
        }))
    }));
    let n = name.clone();
    let remove_control = crate::acting::Control {
        enabled: row.remove.enabled,
        tooltip: if row.remove.enabled {
            None
        } else {
            row.remove.tooltip
        },
    };
    let remove = button(
        SharedString::from(format!("remove-{name}")),
        "Remove",
        ButtonKind::Ghost,
        gate::state(remove_control),
        a,
    );
    let remove = if remove_control.enabled {
        gate::tip_text(remove, row.remove_tip.clone().into(), a)
    } else {
        gate::tip(remove, remove_control, a)
    };
    buttons = buttons.child(remove.when(remove_control.enabled, |b| {
        b.on_click(cx.listener(move |this, _, _, cx| {
            this.ask_remove(&n, cx);
        }))
    }));
    card(a)
        .p(px(12.0))
        .flex()
        .items_start()
        .gap(px(10.0))
        .child(details)
        .child(buttons)
}

fn copy_button(name: &str, copied: bool, a: Appearance, cx: &mut Context<VaultSheetView>) -> Div {
    let n = name.to_owned();
    let button = copy_name::button(
        SharedString::from(format!("copy-name-{name}")),
        copied,
        true,
        a,
    )
    .on_click(cx.listener(move |this, _, _, cx| this.copy_name(&n, cx)));
    div()
        .h(px(0.0))
        .flex()
        .items_center()
        .flex_shrink_0()
        .child(button)
}

pub(super) fn notice(text: &str, ink: Ink, a: Appearance) -> AnyElement {
    scrollable(div().id("vault-notice").p(px(SIDE_PADDING)).child(label(
        text,
        TypeRole::Body,
        ink,
        a,
    )))
    .into_any_element()
}
