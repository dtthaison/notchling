use std::collections::BTreeSet;
use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyElement, Context, Div, Entity, Window};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{RADIUS_COMPOSER, SIDE_PADDING};
use notchling_kit::ui::{hsla, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::ChatView;
use crate::chat::vault_ask::{self, Covers, Placement, Ticket};
use crate::mac::shell::AppVault;
use crate::vault::approval::{pending, PROMPT_SIZE};
use crate::vault::approval_view::VaultApprovalView;
use crate::vault::presence::{Done, Presence};
use crate::vault::prompt_view::{VaultRequestView, REQUEST_SIZE};
use crate::vault::store::Vault;
use crate::vault::twins::Cover;
use crate::vault::wire::PendingRequest;

pub const VAULT_ENV: &str = "NOTCHLING_RENDER_VAULT";
const SHEET_MARGIN: f32 = 80.0;

enum Prompt {
    Approval(Entity<VaultApprovalView>),
    Request(Entity<VaultRequestView>),
}

impl Prompt {
    fn show(&self, presence: Presence, visible: bool, cx: &mut gpui::App) {
        match self {
            Prompt::Approval(view) => view.update(cx, |v, cx| {
                v.set_presence(presence, cx);
                v.set_visible(visible, cx);
            }),
            Prompt::Request(view) => view.update(cx, |v, cx| {
                v.set_presence(presence, cx);
                v.set_visible(visible, cx);
            }),
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
    done: bool,
}

impl Item {
    fn cover<'a>(&self, cx: &'a gpui::App) -> Option<&'a Cover> {
        match &self.prompt {
            Prompt::Request(view) => Some(view.read(cx).cover()),
            Prompt::Approval(_) => None,
        }
    }

    fn covers(&self, request: &PendingRequest, cx: &gpui::App) -> bool {
        self.cover(cx).is_some_and(|mine| mine.covers(request))
    }

    fn stays(&self, ids: &[&str], requests: &[PendingRequest], cx: &gpui::App) -> bool {
        match self.cover(cx) {
            Some(mine) => mine.stays(requests),
            None => ids.contains(&self.ticket.id.as_str()),
        }
    }
}

#[derive(Default)]
pub struct State {
    items: Vec<Item>,
    waiting: BTreeSet<String>,
    seen: Vec<String>,
    preview: bool,
    raised: Option<String>,
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
        let mut tickets = vault_ask::tickets(&self.switches.approvals(), &self.switches.requests());
        tickets.retain(|t| {
            !self
                .vault
                .items
                .iter()
                .any(|i| i.done && i.ticket.id == t.id)
        });
        tickets
    }

    fn vault_live(&self) -> impl Iterator<Item = &Item> {
        self.vault.items.iter().filter(|i| !i.done)
    }

    fn vault_cards_for(&self, row: &str) -> Vec<&Item> {
        self.vault_live()
            .filter(|i| self.vault_place(i) == Placement::Card(row.to_owned()))
            .collect()
    }

    fn vault_card_front(&self) -> Option<&Item> {
        let row = self.selected.as_deref()?;
        let items = self.vault_cards_for(row);
        let ids: Vec<&str> = items.iter().map(|i| i.ticket.id.as_str()).collect();
        let at = vault_ask::pick(&ids, self.vault.raised.as_deref())?;
        items.get(at).copied()
    }

    fn vault_covers(&self, window: &Window) -> Covers {
        Covers {
            active: window.is_window_active(),
            sheet: self.sheet.is_some(),
            inbox: self.inbox_panel.is_some(),
            phone: self.phone.is_some(),
            subagent: self.open_subagent.is_some(),
            terminal: self.terminal.shown() && self.terminal.expanded && !self.page.is_open(),
            vault_sheet: self.vault_sheet_item().is_some(),
        }
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
            self.vault_live()
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
        let covers = self.vault_covers(window);
        let sheet = self.vault_sheet_item().map(|i| i.ticket.id.clone());
        let card = self.vault_card_front().map(|i| i.ticket.id.clone());
        for item in &self.vault.items {
            let placement = self.vault_place(item);
            let (presence, front) = match placement {
                Placement::Card(_) => (
                    Presence::Card,
                    card.as_deref() == Some(item.ticket.id.as_str()),
                ),
                Placement::Sheet => (
                    Presence::Sheet,
                    sheet.as_deref() == Some(item.ticket.id.as_str()),
                ),
            };
            let visible = !item.done
                && vault_ask::visible(&placement, self.selected.as_deref(), front, &covers);
            item.prompt.show(presence, visible, cx);
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
        self.vault.items.retain(|i| i.stays(&ids, &requests, cx));
        if let Some(raised) = &self.vault.raised {
            if !self.vault.items.iter().any(|i| &i.ticket.id == raised) {
                self.vault.raised = None;
            }
        }
        let vault = cx
            .try_global::<AppVault>()
            .map(|v| v.0.clone())
            .unwrap_or_else(|| Arc::new(Vault::fixed(Vec::new())));
        for approval in approvals {
            if self.vault.items.iter().any(|i| i.ticket.id == approval.id) {
                continue;
            }
            let (feed, vault) = (self.switches.clone(), vault.clone());
            let entity = cx.new(|cx| {
                VaultApprovalView::live_in(&approval, feed, vault, Presence::Sheet, window, cx)
            });
            let id = approval.id.clone();
            cx.subscribe(&entity, move |view, _, _: &Done, cx| {
                view.vault_done(&id, cx)
            })
            .detach();
            self.vault.items.push(Item {
                ticket: Ticket {
                    id: approval.id.clone(),
                    session: approval.session.clone(),
                    cwd: approval.verified.as_ref().and_then(|v| v.cwd.clone()),
                },
                prompt: Prompt::Approval(entity),
                forced: None,
                done: false,
            });
        }
        for request in requests {
            if self
                .vault
                .items
                .iter()
                .any(|i| i.ticket.id == request.id || i.covers(&request, cx))
            {
                continue;
            }
            let (feed, vault) = (self.switches.clone(), vault.clone());
            let ticket = Ticket {
                id: request.id.clone(),
                session: request.session.clone(),
                cwd: request.verified.as_ref().and_then(|v| v.cwd.clone()),
            };
            let entity = cx.new(|cx| {
                VaultRequestView::live_in(feed, vault, request, Presence::Sheet, window, cx)
            });
            let id = ticket.id.clone();
            cx.subscribe(&entity, move |view, _, _: &Done, cx| {
                view.vault_done(&id, cx)
            })
            .detach();
            self.vault.items.push(Item {
                ticket,
                prompt: Prompt::Request(entity),
                forced: None,
                done: false,
            });
        }
    }

    fn vault_done(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(item) = self.vault.items.iter_mut().find(|i| i.ticket.id == id) {
            item.done = true;
        }
        self.vault_tick(cx);
        cx.notify();
    }

    pub fn show_vault(&mut self, ids: &[String], window: &mut Window, cx: &mut Context<Self>) {
        self.sync_vault(window, cx);
        let raised = ids.iter().find_map(|id| self.vault_item_for(id, cx));
        let Some(raised) = raised else {
            cx.notify();
            return;
        };
        let placement = self
            .vault_live()
            .find(|i| i.ticket.id == raised)
            .map(|i| self.vault_place(i));
        self.vault.raised = Some(raised);
        self.inbox_panel = None;
        self.phone = None;
        self.sheet = None;
        self.terminal.expanded = false;
        if self.open_subagent.is_some() {
            self.close_subagent(cx);
        }
        if let Some(Placement::Card(row)) = placement {
            self.select(&row, cx);
        }
        self.sync_vault(window, cx);
        self.vault_tick(cx);
        cx.notify();
    }

    fn vault_item_for(&self, id: &str, cx: &gpui::App) -> Option<String> {
        let asked: Option<PendingRequest> = self
            .switches
            .requests()
            .iter()
            .filter_map(PendingRequest::from_event)
            .find(|r| r.id == id);
        self.vault_live()
            .find(|i| i.ticket.id == id || asked.as_ref().is_some_and(|asked| i.covers(asked, cx)))
            .map(|i| i.ticket.id.clone())
    }

    pub(super) fn vault_sheet_shown(&self) -> bool {
        self.vault_sheet_item().is_some()
    }

    fn vault_sheet_item(&self) -> Option<&Item> {
        let sheets: Vec<&Item> = self
            .vault_live()
            .filter(|i| self.vault_place(i) == Placement::Sheet)
            .collect();
        let ids: Vec<&str> = sheets.iter().map(|i| i.ticket.id.as_str()).collect();
        let at = vault_ask::pick(&ids, self.vault.raised.as_deref())?;
        sheets.get(at).copied()
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
                    cwd: None,
                },
                prompt,
                forced: Some(forced),
                done: false,
            }],
            waiting: BTreeSet::new(),
            seen: Vec::new(),
            preview: true,
            raised: None,
        }
    }
}

pub(super) fn card_frame(a: Appearance) -> Div {
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
    let items = view.vault_cards_for(selected);
    let Some(front) = view.vault_card_front() else {
        return Vec::new();
    };
    let at = items
        .iter()
        .position(|i| i.ticket.id == front.ticket.id)
        .unwrap_or(0);
    let mut out = Vec::new();
    if items.len() > 1 {
        out.push(div().mx(px(SIDE_PADDING)).mb(px(4.0)).child(label(
            &format!("{}/{}", at + 1, items.len()),
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        )));
    }
    out.push(card_frame(a).child(front.prompt.element()).part_at(
        "Vault Card",
        Corner::TopTrailing,
        a,
        cx,
    ));
    out
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
