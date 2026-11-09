//! One row of the vault sheet's list over GPUI (macOS): the kind, name, URL, variables, note and
//! expiry of a saved secret (never its value), with Renew…, Edit and Remove.

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::part::ButtonKind;
use notchling_kit::ui::{button, card, label, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::form::Form;
use super::list::{Row, RENEW_TIP};
use super::view::VaultSheetView;
use crate::mac::gate;

pub(super) fn list_row(row: &Row, a: Appearance, cx: &mut Context<VaultSheetView>) -> Div {
    let name = row.name.clone();
    let mut tags = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(label(&row.name, TypeRole::Heading, Ink::Text, a))
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
            this.removing = Some(n.clone());
            cx.notify();
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
