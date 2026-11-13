use std::rc::Rc;
use std::time::Duration;

use gpui::{div, prelude::*, px, Context, Div, Stateful};
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, icon_button, KitStyled};
use notchling_kit::{Appearance, Icon, IconSize, Ink};

use super::cost::{cost_view, Range};
use super::model::{Tab, CLAUDE_CODE, REFRESH_TIP};
use super::store::Snapshot;
use super::view::AccountsView;
use super::view_cost::cost_page;
use super::view_rows::segmented;
use crate::acting::Control;
use crate::mac::gate;
use crate::sheets::view::Dismiss;

impl AccountsView {
    pub(super) fn poll_add(&self, cx: &mut Context<Self>) {
        if !self.store.claim_add_poll() {
            return;
        }
        let (store, switches) = (self.store.clone(), self.switches());
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let s = store.clone();
                let again = cx
                    .background_executor()
                    .spawn(async move { s.add_poll_step(&switches) })
                    .await;
                let shown = this.update(cx, |_, cx| cx.notify()).is_ok();
                if !again || !shown {
                    break;
                }
            }
            let s = store.clone();
            cx.background_executor()
                .spawn(async move { s.end_add_poll(&switches) })
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    pub(super) fn header(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let tabs: Vec<(String, String)> = Tab::ALL
            .iter()
            .map(|t| (t.title().to_owned(), t.title().to_owned()))
            .collect();
        let pick: super::view_rows::Handler<Self, String> = Rc::new(|this, key: String, cx| {
            if let Some(tab) = Tab::ALL.iter().find(|t| t.title() == key) {
                this.tab = *tab;
                this.refresh(cx);
                cx.notify();
            }
        });
        let refresh_control = Control {
            enabled: self.store.is_live(),
            tooltip: Some(REFRESH_TIP),
        };
        let refresh = gate::tip(
            icon_button(
                "accounts-refresh",
                Icon::Resume,
                IconSize::Button.points(),
                ButtonKind::Ghost,
                gate::state(refresh_control),
                a,
            ),
            refresh_control,
            a,
        )
        .when(refresh_control.enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| {
                this.refresh_all(cx);
                cx.notify();
            }))
        });
        let done = self.in_sheet.then(|| {
            button(
                "accounts-done",
                "Done",
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss)))
        });
        div()
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .py(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .fill(Ink::Card, a)
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(div().flex_1())
            .child(segmented(
                "accounts-tab",
                &tabs,
                self.tab.title(),
                a,
                cx,
                pick,
            ))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(10.0))
                    .child(refresh)
                    .children(done),
            )
    }

    pub(super) fn cost_tab(
        &self,
        s: &Snapshot,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let emails = email_index(s);
        let name = move |id: &str| emails.get(id).cloned();
        let view = cost_view(s.cost.as_ref(), self.range, &self.account, &name);
        let on_range: super::view_rows::Handler<Self, Range> = Rc::new(|this, range, cx| {
            this.range = range;
            this.refresh(cx);
            cx.notify();
        });
        let on_account: super::view_rows::Handler<Self, String> = Rc::new(|this, id, cx| {
            this.account = id;
            cx.notify();
        });
        cost_page(&view, a, cx, on_range, on_account)
    }
}

pub(super) fn row_identity(s: &Snapshot, provider: &str, id: &str) -> (String, bool) {
    if provider == CLAUDE_CODE {
        let email = s
            .claude
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.email.clone())
            .unwrap_or_default();
        return (email, s.claude.active.as_deref() == Some(id));
    }
    s.providers
        .providers
        .iter()
        .find(|p| p.id == provider)
        .map(|p| {
            let email = p
                .accounts
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.email.clone())
                .unwrap_or_default();
            (email, p.active_id.as_deref() == Some(id))
        })
        .unwrap_or_default()
}

pub(super) fn sign_in_url(s: &Snapshot, provider: &str) -> Option<String> {
    if provider == CLAUDE_CODE {
        return s.add.as_ref().and_then(|a| a.url.clone());
    }
    s.providers
        .providers
        .iter()
        .find(|p| p.id == provider)
        .and_then(|p| p.sign_in_url.clone())
}

pub(super) fn email_index(s: &Snapshot) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for a in &s.claude.accounts {
        map.insert(a.id.clone(), a.email.clone());
    }
    for p in &s.providers.providers {
        for a in &p.accounts {
            map.entry(a.id.clone()).or_insert_with(|| a.email.clone());
        }
    }
    map
}
