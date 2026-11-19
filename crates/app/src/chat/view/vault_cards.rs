use std::collections::BTreeSet;
use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyElement, Context, Div, Entity, Window};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{RADIUS_COMPOSER, SIDE_PADDING};
use notchling_kit::ui::{hsla, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink};

use super::ChatView;
use crate::chat::vault_ask::{self, Placement, Ticket};
use crate::mac::shell::AppVault;
use crate::vault::approval::{pending, PROMPT_SIZE};
use crate::vault::approval_view::VaultApprovalView;
use crate::vault::presence::Presence;
use crate::vault::prompt_view::{VaultRequestView, REQUEST_SIZE};
use crate::vault::store::Vault;
use crate::vault::wire::PendingRequest;

pub const VAULT_ENV: &str = "NOTCHLING_RENDER_VAULT";
const SHEET_MARGIN: f32 = 80.0;

enum Prompt {
    Approval(Entity<VaultApprovalView>),
    Request(Entity<VaultRequestView>),
}

impl Prompt {
    fn show(&self, presence: Presence, visible: bool, active: bool, cx: &mut gpui::App) {
        match self {
            Prompt::Approval(view) => view.update(cx, |v, cx| {
                v.set_presence(presence, cx);
                v.set_visible(visible, active, cx);
            }),
            Prompt::Request(view) => view.update(cx, |v, cx| v.set_presence(presence, cx)),
        }
    }

    fn element(&self) -> AnyElement {
        match self {
            Prompt::Approval(view) => view.clone().into_any_element(),
            Prompt::Request(view) => view.clone().into_any_element(),
        }
    }

    fn sheet_size(&self) -> (f32, f32) {
        match self {
            Prompt::Approval(_) => PROMPT_SIZE,
            Prompt::Request(_) => REQUEST_SIZE,
        }
    }
}

struct Item {
    ticket: Ticket,
    prompt: Prompt,
    forced: Option<Placement>,
}

#[derive(Default)]
pub struct State {
    items: Vec<Item>,
    waiting: BTreeSet<String>,
    seen: Vec<String>,
    preview: bool,
}

impl State {
    pub(super) fn waiting(&self) -> &BTreeSet<String> {
        &self.waiting
    }
}

impl ChatView {
    fn vault_tickets(&self) -> Vec<Ticket> {
        if self.vault.preview {
            return self.vault.items.iter().map(|i| i.ticket.clone()).collect();
        }
        vault_ask::tickets(&self.switches.approvals(), &self.switches.requests())
    }

    fn vault_place(&self, item: &Item) -> Placement {
        match (&item.forced, self.selected.as_ref()) {
            (Some(Placement::Card(_)), Some(row)) => Placement::Card(row.clone()),
            (Some(forced), _) => forced.clone(),
            (None, _) => vault_ask::place(&item.ticket, &self.sessions),
        }
    }

    pub(super) fn vault_tick(&mut self, cx: &mut Context<Self>) {
        let tickets = self.vault_tickets();
        let ids: Vec<String> = tickets.iter().map(|t| t.id.clone()).collect();
        let waiting: BTreeSet<String> = if self.vault.preview {
            self.vault
                .items
                .iter()
                .filter_map(|i| match self.vault_place(i) {
                    Placement::Card(row) => Some(row),
                    Placement::Sheet => None,
                })
                .collect()
        } else {
            vault_ask::waiting_rows(&tickets, &self.sessions)
        };
        if ids != self.vault.seen || waiting != self.vault.waiting {
            self.vault.seen = ids;
            self.vault.waiting = waiting;
            cx.notify();
        }
    }

    pub(super) fn sync_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.vault.preview {
            self.vault_entities(window, cx);
        }
        let sheet = self.vault_sheet_item().map(|i| i.ticket.id.clone());
        let active = window.is_window_active();
        let elsewhere = self.open_subagent.is_some();
        for item in &self.vault.items {
            let (presence, visible) = match self.vault_place(item) {
                Placement::Card(row) => (
                    Presence::Card,
                    !elsewhere && self.selected.as_deref() == Some(row.as_str()),
                ),
                Placement::Sheet => (
                    Presence::Sheet,
                    sheet.as_deref() == Some(item.ticket.id.as_str()),
                ),
            };
            item.prompt.show(presence, visible, active, cx);
        }
    }

    fn vault_entities(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let approvals = pending(&self.switches.approvals());
        let requests: Vec<PendingRequest> = self
            .switches
            .requests()
            .iter()
            .filter_map(PendingRequest::from_event)
            .collect();
        let ids: Vec<&str> = approvals
            .iter()
            .map(|a| a.id.as_str())
            .chain(requests.iter().map(|r| r.id.as_str()))
            .collect();
        self.vault
            .items
            .retain(|i| ids.contains(&i.ticket.id.as_str()));
        let vault = cx
            .try_global::<AppVault>()
            .map(|v| v.0.clone())
            .unwrap_or_else(|| Arc::new(Vault::fixed(Vec::new())));
        for approval in approvals {
            if self.vault.items.iter().any(|i| i.ticket.id == approval.id) {
                continue;
            }
            let (feed, vault) = (self.switches.clone(), vault.clone());
            let entity = cx.new(|cx| VaultApprovalView::live(&approval, feed, vault, window, cx));
            self.vault.items.push(Item {
                ticket: Ticket {
                    id: approval.id.clone(),
                    session: approval.session.clone(),
                },
                prompt: Prompt::Approval(entity),
                forced: None,
            });
        }
        for request in requests {
            if self.vault.items.iter().any(|i| i.ticket.id == request.id) {
                continue;
            }
            let (feed, vault) = (self.switches.clone(), vault.clone());
            let ticket = Ticket {
                id: request.id.clone(),
                session: request.session.clone(),
            };
            let entity = cx.new(|cx| VaultRequestView::live(feed, vault, request, cx));
            self.vault.items.push(Item {
                ticket,
                prompt: Prompt::Request(entity),
                forced: None,
            });
        }
    }

    pub fn show_vault(&mut self, ids: &[String], window: &mut Window, cx: &mut Context<Self>) {
        self.sync_vault(window, cx);
        self.vault_tick(cx);
        let row = ids.iter().find_map(|id| {
            let item = self.vault.items.iter().find(|i| &i.ticket.id == id)?;
            match self.vault_place(item) {
                Placement::Card(row) => Some(row),
                Placement::Sheet => None,
            }
        });
        if let Some(row) = row {
            self.inbox_panel = None;
            if self.open_subagent.is_some() {
                self.close_subagent(cx);
            }
            self.select(&row, cx);
        }
        cx.notify();
    }

    pub(super) fn vault_sheet_shown(&self) -> bool {
        self.vault_sheet_item().is_some()
    }

    fn vault_sheet_item(&self) -> Option<&Item> {
        self.vault
            .items
            .iter()
            .find(|i| self.vault_place(i) == Placement::Sheet)
    }
}

impl State {
    pub fn preview(kind: &str, cx: &mut gpui::App) -> State {
        let sheet = kind.ends_with("-sheet");
        let forced = if sheet {
            Placement::Sheet
        } else {
            Placement::Card(String::new())
        };
        let presence = if sheet {
            Presence::Sheet
        } else {
            Presence::Card
        };
        let prompt = if kind.starts_with("request") {
            let pending = crate::vault::renders::pending_request();
            Prompt::Request(cx.new(|cx| {
                VaultRequestView::fixed(pending, crate::vault::renders::TODAY, cx)
                    .with_presence(presence)
            }))
        } else {
            let approval = pending(&crate::vault::renders::approvals())
                .into_iter()
                .next();
            let Some(approval) = approval else {
                return State::default();
            };
            Prompt::Approval(cx.new(|cx| VaultApprovalView::fixed_in(&approval, presence, cx)))
        };
        State {
            items: vec![Item {
                ticket: Ticket {
                    id: "preview".into(),
                    session: None,
                },
                prompt,
                forced: Some(forced),
            }],
            waiting: BTreeSet::new(),
            seen: Vec::new(),
            preview: true,
        }
    }
}

fn card_frame(a: Appearance) -> Div {
    div()
        .mx(px(SIDE_PADDING))
        .mb(px(8.0))
        .rounded(px(RADIUS_COMPOSER))
        .fill(Ink::Card, a)
        .border(px(1.0))
        .border_color(hsla(Ink::Accent.color(a).with_alpha(0.45)))
        .overflow_hidden()
}

pub(super) fn cards(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Vec<Div> {
    if view.open_subagent.is_some() {
        return Vec::new();
    }
    let Some(selected) = view.selected.as_deref() else {
        return Vec::new();
    };
    view.vault
        .items
        .iter()
        .filter(|item| view.vault_place(item) == Placement::Card(selected.to_owned()))
        .map(|item| {
            card_frame(a).child(item.prompt.element()).part_at(
                "Vault Card",
                Corner::TopTrailing,
                a,
                cx,
            )
        })
        .collect()
}

pub(super) fn sheet(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<Div> {
    let item = view.vault_sheet_item()?;
    let (width, height) = item.prompt.sheet_size();
    let room = f32::from(window.viewport_size().height) - SHEET_MARGIN;
    Some(
        div()
            .absolute()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(Ink::Shadow.color(a)))
            .child(
                div()
                    .w(px(width))
                    .h(px(height.min(room.max(240.0))))
                    .rounded(px(14.0))
                    .border(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .fill(Ink::Window, a)
                    .overflow_hidden()
                    .child(item.prompt.element())
                    .part_at("Vault Sheet", Corner::TopTrailing, a, cx),
            ),
    )
}
