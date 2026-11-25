use std::time::Duration;

use gpui::{Context, Div, SharedString, Stateful};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::icon_button_small;
use notchling_kit::{Appearance, Icon};
use notchling_locale as t;

use super::reveal::AUTO_HIDE_MS;
use crate::mac::gate;

const WATCH_EVERY: Duration = Duration::from_secs(1);

pub fn eye_button(id: SharedString, shown: bool, usable: bool, a: Appearance) -> Stateful<Div> {
    let glyph = if shown { Icon::EyeOff } else { Icon::Eye };
    let state = if usable {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    let button = icon_button_small(id, glyph, ButtonKind::Ghost, state, a);
    gate::tip_text(
        button,
        if shown {
            t::consent::card_vault_hide()
        } else {
            t::consent::card_vault_show()
        }
        .into(),
        a,
    )
}

pub fn notify_when_hidden<V: 'static>(cx: &mut Context<V>) {
    cx.spawn(async move |this, cx| {
        let mut waited = 0;
        while waited < AUTO_HIDE_MS as u64 + 50 {
            cx.background_executor().timer(WATCH_EVERY).await;
            waited += WATCH_EVERY.as_millis() as u64;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }
    })
    .detach();
}
