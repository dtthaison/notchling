use gpui::{div, prelude::*, px, Context, Div, SharedString, Window};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, label, menu_note, menu_panel, menu_rule, MenuItem};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::pick::{using_line, VaultPick};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickEvent {
    Toggle,
    Choose(String),
    Clear,
    New,
}

pub type PickHandler<V> = fn(&mut V, PickEvent, &mut Window, &mut Context<V>);

pub fn pick_block<V: 'static>(
    key: &str,
    pick: &VaultPick,
    chosen: Option<&str>,
    can_new: bool,
    a: Appearance,
    cx: &mut Context<V>,
    handler: PickHandler<V>,
) -> Div {
    let mut block = div().flex().flex_col().gap(px(SPACE[0]));
    let mut row = div().flex().items_center().gap(px(SPACE[1])).child(
        button(
            SharedString::from(format!("{key}-vault-use")),
            t::cluster::vault_use(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(
            cx.listener(move |this, _, window, cx| handler(this, PickEvent::Toggle, window, cx)),
        ),
    );
    if chosen.is_some() {
        row = row.child(
            button(
                SharedString::from(format!("{key}-vault-stop")),
                t::cluster::vault_stop(),
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(
                cx.listener(move |this, _, window, cx| handler(this, PickEvent::Clear, window, cx)),
            ),
        );
    }
    block = block.child(row);
    if let Some(name) = chosen {
        block = block.child(label(&using_line(name), TypeRole::Meta, Ink::TextMuted, a));
    }
    if pick.is_open() {
        block = block.child(list(key, pick, chosen, can_new, a, cx, handler));
    }
    block
}

fn list<V: 'static>(
    key: &str,
    pick: &VaultPick,
    chosen: Option<&str>,
    can_new: bool,
    a: Appearance,
    cx: &mut Context<V>,
    handler: PickHandler<V>,
) -> Div {
    let mut panel = menu_panel(a).w_full();
    for row in pick.rows(chosen) {
        let id = row.id.clone();
        panel = panel.child(
            MenuItem::new(SharedString::from(format!("{key}-vault-{id}")), row.name)
                .detail(&row.detail)
                .inline()
                .checked(row.checked)
                .build(a)
                .on_click(cx.listener(move |this, _, window, cx| {
                    handler(this, PickEvent::Choose(id.clone()), window, cx)
                })),
        );
    }
    if let Some(note) = pick.note() {
        panel = panel.child(menu_note(&note.text(), a));
    }
    if can_new {
        panel = panel.child(menu_rule(a)).child(
            MenuItem::new(
                SharedString::from(format!("{key}-vault-new")),
                t::cluster::vault_new(),
            )
            .build(a)
            .on_click(
                cx.listener(move |this, _, window, cx| handler(this, PickEvent::New, window, cx)),
            ),
        );
    }
    panel
}
