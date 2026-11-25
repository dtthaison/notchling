use std::time::Duration;

use gpui::{ClipboardItem, Context, Div, SharedString, Stateful};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::icon_button_small;
use notchling_kit::{Appearance, Icon};
use notchling_locale as t;

use crate::chat::message_meta::{copied_shows, COPIED_MS};
use crate::mac::gate;

pub type Copied = Option<(String, i64)>;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn shows(copied: &Copied, name: &str) -> bool {
    copied
        .as_ref()
        .is_some_and(|(copied, at)| copied == name && copied_shows(*at, now_ms()))
}

pub fn copy<V: 'static>(copied: &mut Copied, name: &str, cx: &mut Context<V>) {
    cx.write_to_clipboard(ClipboardItem::new_string(name.to_owned()));
    mark(copied, name, cx);
}

pub fn mark<V: 'static>(copied: &mut Copied, key: &str, cx: &mut Context<V>) {
    *copied = Some((key.to_owned(), now_ms()));
    cx.spawn(async move |this, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(COPIED_MS as u64 + 50))
            .await;
        let _ = this.update(cx, |_, cx| cx.notify());
    })
    .detach();
    cx.notify();
}

pub fn button(id: SharedString, copied: bool, usable: bool, a: Appearance) -> Stateful<Div> {
    button_tip(id, copied, usable, t::consent::card_form_copy_name(), a)
}

pub fn button_tip(
    id: SharedString,
    copied: bool,
    usable: bool,
    tip: &'static str,
    a: Appearance,
) -> Stateful<Div> {
    let glyph = if copied { Icon::Check } else { Icon::Copy };
    let state = if usable {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    let button = icon_button_small(id, glyph, ButtonKind::Ghost, state, a);
    gate::tip_text(
        button,
        if copied { t::chat::copy_done() } else { tip }.into(),
        a,
    )
}
