use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, AnyView, Context, Div, Entity, Render, SharedString, Stateful, Window,
};
use notchling_engine::inbox::ActionId;
use notchling_kit::metric::{SIDE_PADDING, SPACE};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon, label, scrollable, window_appearance, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};
use serde_json::Value;

use super::consent_card::{vault_moved, ConsentCard, Env};
use super::consent_view::{buttons, card_body};
use super::feed::InboxFeed;
use super::model::{is_local, target, Row, Target};
use super::renders::{consent_model, model, shown, NOW_MS};
use super::{MAX_HEIGHT, WIDTH};
use crate::phone::bridge::now_ms;

const POLL: Duration = Duration::from_millis(500);
const CLOCK_EVERY: u32 = 20;
const ALLOW_SETTLED_MS: i64 = 60_000;

pub struct InboxPanelView {
    feed: Arc<InboxFeed>,
    fixed_now: Option<i64>,
    done_open: bool,
    sent_seen: HashSet<String>,
    busy: HashSet<String>,
    note: Option<String>,
    drawn: u64,
    shown: HashMap<String, Instant>,
    fixed_shown_ms: Option<i64>,
    vault_on: Option<bool>,
    vault_seen: Option<(u64, bool)>,
    cards_open: bool,
    allow_waiting: bool,
}

impl InboxPanelView {
    pub fn live(feed: Arc<InboxFeed>, cx: &mut Context<Self>) -> InboxPanelView {
        let mut view = InboxPanelView {
            feed,
            fixed_now: None,
            done_open: false,
            sent_seen: HashSet::new(),
            busy: HashSet::new(),
            note: None,
            drawn: 0,
            shown: HashMap::new(),
            fixed_shown_ms: None,
            vault_on: None,
            vault_seen: None,
            cards_open: false,
            allow_waiting: false,
        };
        view.send_seen(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(POLL).await;
                ticks = ticks.wrapping_add(1);
                if this.update(cx, |view, cx| view.poll(ticks, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        view
    }

    pub fn fixed(feed: Arc<InboxFeed>, now_ms: i64, done_open: bool) -> InboxPanelView {
        InboxPanelView {
            feed,
            fixed_now: Some(now_ms),
            done_open,
            sent_seen: HashSet::new(),
            busy: HashSet::new(),
            note: None,
            drawn: 0,
            shown: HashMap::new(),
            fixed_shown_ms: Some(ALLOW_SETTLED_MS),
            vault_on: Some(true),
            vault_seen: None,
            cards_open: false,
            allow_waiting: false,
        }
    }

    pub fn with_vault(mut self, on: bool) -> InboxPanelView {
        self.vault_on = Some(on);
        self
    }

    fn follow_vault(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(feed) = cx.try_global::<crate::mac::shell::AppSwitchFeed>() else {
            return false;
        };
        let now = (
            feed.0.changes().vault,
            feed.0.switches().is_on(crate::switches::Switch::Vault),
        );
        let moved = vault_moved(self.vault_seen, now);
        self.vault_seen = Some(now);
        if moved {
            self.feed.refresh_consents();
        }
        moved
    }

    fn poll(&mut self, ticks: u32, cx: &mut Context<Self>) {
        let moved = self.follow_vault(cx);
        let changed = moved || self.feed.generation() != self.drawn;
        let counting = self.cards_open && ticks.is_multiple_of(2);
        if changed || counting || self.allow_waiting || ticks.is_multiple_of(CLOCK_EVERY) {
            cx.notify();
        }
        if changed {
            self.note = None;
            self.send_seen(cx);
        }
    }

    fn send_seen(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .feed
            .snapshot(0)
            .unseen_ids()
            .into_iter()
            .filter(|id| !self.sent_seen.contains(id))
            .collect();
        if ids.is_empty() || !self.feed.is_live() {
            return;
        }
        self.sent_seen.extend(ids.iter().cloned());
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let sent_ids = ids.clone();
            let sent = cx
                .background_executor()
                .spawn(async move { feed.seen(&sent_ids) })
                .await;
            if let Err(e) = sent {
                eprintln!("inbox: marking rows seen wasn't sent ({e})");
                this.update(cx, |view, _| {
                    for id in &ids {
                        view.sent_seen.remove(id);
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    fn bring_up(&mut self, open: Option<&Value>, cx: &mut Context<Self>) {
        match open.and_then(target) {
            Some(Target::Approval(ticket)) | Some(Target::Request(ticket)) => {
                crate::mac::vault_prompts::raise(&ticket);
            }
            Some(Target::Settings { page, .. }) if page == "agents" => {
                cx.defer(crate::mac::shell::open_chat_settings_agents);
            }
            Some(Target::Machines(_)) => {
                self.note = Some("Open Machines from the menu bar.".to_string());
                cx.notify();
            }
            Some(Target::Ask { id, session }) => {
                cx.defer(move |cx| crate::mac::shell::open_chat_ask(cx, &session, &id));
            }
            Some(Target::Settings { .. }) | None => {}
        }
    }

    fn act(&mut self, id: String, action: ActionId, open: Option<Value>, cx: &mut Context<Self>) {
        if is_local(action) {
            self.bring_up(open.as_ref(), cx);
            return;
        }
        if !self.busy.insert(id.clone()) {
            return;
        }
        self.note = None;
        let feed = self.feed.clone();
        let sent = id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { feed.answer(&sent, action) })
                .await;
            this.update(cx, |view, cx| {
                view.busy.remove(&id);
                if let Err(e) = result {
                    view.note = Some(e);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn consent_act(&mut self, id: String, allow: bool, app_applied: bool, cx: &mut Context<Self>) {
        if !self.busy.insert(id.clone()) {
            return;
        }
        self.note = None;
        let feed = self.feed.clone();
        let sent = id.clone();
        let open_form = allow && app_applied;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if open_form {
                        feed.consent_opened(&sent).map(|_| true)
                    } else {
                        feed.consent_answer(&sent, allow)
                    }
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy.remove(&id);
                match result {
                    Ok(_) if open_form => {
                        if let Some(feed) = cx.try_global::<crate::mac::shell::AppSwitchFeed>() {
                            feed.0.allow_request(&id);
                        }
                        crate::mac::vault_prompts::raise(&id);
                    }
                    Ok(_) => {}
                    Err(e) => view.note = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle_done(&mut self, cx: &mut Context<Self>) {
        self.done_open = !self.done_open;
        cx.notify();
    }

    fn action_buttons(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        let live = row.enabled && !self.busy.contains(&row.id);
        let mut buttons = div().flex().flex_wrap().gap(px(SPACE[1]));
        for (action, text, primary) in &row.actions {
            let kind = if *primary && row.enabled {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            let state = if live {
                PartState::Rest
            } else {
                PartState::Disabled
            };
            let id = SharedString::from(format!("inbox-{}-{action:?}", row.id));
            let mut one = button(id, text, kind, state, a);
            if live {
                let (row_id, action, open) = (row.id.clone(), *action, row.open.clone());
                one = one.on_click(cx.listener(move |this, _, _, cx| {
                    this.act(row_id.clone(), action, open.clone(), cx);
                }));
            }
            buttons = buttons.child(one);
        }
        buttons
    }

    fn body(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let ink = if row.enabled {
            Ink::Text
        } else {
            Ink::TextMuted
        };
        let top = div()
            .flex()
            .items_start()
            .gap(px(SPACE[1]))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(label(&row.title, TypeRole::Heading, ink, a).elide()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .child(label(&row.ago, TypeRole::Meta, Ink::TextFaint, a)),
            );
        let mut body = div()
            .id(SharedString::from(format!("inbox-body-{}", row.id)))
            .flex()
            .flex_col()
            .gap(px(SPACE[0] / 2.0))
            .child(top)
            .children((!row.detail.is_empty()).then(|| {
                label(&row.detail, TypeRole::Meta, Ink::TextMuted, a)
                    .w_full()
                    .elide()
            }));
        if row.enabled && row.open.as_ref().and_then(target).is_some() {
            let open = row.open.clone();
            body = body
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.bring_up(open.as_ref(), cx);
                    cx.notify();
                }));
        }
        body
    }

    fn row(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .px(px(SPACE[2]))
            .py(px(SPACE[1] + SPACE[0] / 2.0))
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(self.body(row, a, cx))
            .children((!row.actions.is_empty()).then(|| self.action_buttons(row, a, cx)))
            .part("Inbox Row", a, cx)
    }

    fn consent_row(
        &self,
        row: &Row,
        card: &ConsentCard,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = self.busy.contains(&card.id);
        let bar = buttons(card, busy, a);
        let mut allow = bar.allow;
        let mut deny = bar.deny;
        if card.allow.enabled() && !busy {
            let (id, app_applied) = (card.id.clone(), card.app_applied);
            allow = allow.on_click(cx.listener(move |this, _, _, cx| {
                this.consent_act(id.clone(), true, app_applied, cx);
            }));
        }
        if card.answerable && !busy {
            let (id, app_applied) = (card.id.clone(), card.app_applied);
            deny = deny.on_click(cx.listener(move |this, _, _, cx| {
                this.consent_act(id.clone(), false, app_applied, cx);
            }));
        }
        div()
            .px(px(SPACE[2]))
            .py(px(SPACE[1] + SPACE[0] / 2.0))
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(card_body(row, card, a))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(SPACE[1]))
                    .child(allow)
                    .child(deny),
            )
            .part("Consent Card", a, cx)
    }

    fn cards(
        &mut self,
        snapshot: &super::model::InboxModel,
        rows: &[Row],
        now: i64,
        cx: &mut Context<Self>,
    ) -> HashMap<String, ConsentCard> {
        let vault_on = self.vault_on.unwrap_or_else(|| {
            cx.try_global::<crate::mac::shell::AppSwitchFeed>()
                .is_some_and(|f| f.0.switches().is_on(crate::switches::Switch::Vault))
        });
        let mut out = HashMap::new();
        for row in rows.iter().filter(|r| r.enabled) {
            let Some(id) = row.open.as_ref().and_then(super::model::consent_of) else {
                continue;
            };
            let Some(view) = snapshot.consent(id) else {
                continue;
            };
            let first = *self.shown.entry(id.to_owned()).or_insert_with(Instant::now);
            let shown_ms = self
                .fixed_shown_ms
                .unwrap_or_else(|| first.elapsed().as_millis() as i64);
            let env = Env {
                now_ms: now,
                vault_on,
                shown_ms,
                node: row.node.clone(),
            };
            if let Some(card) = ConsentCard::build(view, &env) {
                out.insert(row.id.clone(), card);
            }
        }
        self.cards_open = !out.is_empty();
        self.allow_waiting = out
            .values()
            .any(|c| matches!(c.allow, super::consent_card::Allow::Wait(_)));
        out
    }

    fn heading(&self, text: &str, a: Appearance) -> Div {
        div()
            .px(px(SPACE[2]))
            .pt(px(SPACE[1]))
            .pb(px(SPACE[0]))
            .child(label(text, TypeRole::Label, Ink::TextFaint, a))
    }

    fn done_heading(&self, count: usize, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let chevron = if self.done_open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        div()
            .id("inbox-done")
            .cursor_pointer()
            .px(px(SPACE[2]))
            .pt(px(SPACE[1]))
            .pb(px(SPACE[0]))
            .flex()
            .items_center()
            .gap(px(SPACE[0]))
            .child(icon(chevron, IconSize::Row.points(), Ink::TextFaint, a))
            .child(label("Done", TypeRole::Label, Ink::TextFaint, a))
            .child(label(&count.to_string(), TypeRole::Meta, Ink::TextFaint, a))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_done(cx)))
    }
}

impl Render for InboxPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        self.drawn = self.feed.generation();
        let now = self.fixed_now.unwrap_or_else(now_ms);
        let snapshot = self.feed.snapshot(now);
        let (needs, done) = snapshot.sections();
        let cards = self.cards(&snapshot, &needs, now, cx);
        let mut list = div()
            .id("inbox-list")
            .min_h(px(0.0))
            .pb(px(SPACE[1]))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if needs.is_empty() {
            list = list.child(div().px(px(SPACE[2])).py(px(SPACE[1])).child(label(
                "Nothing needs you",
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )));
        } else {
            list = list.child(self.heading("Needs You", a));
            for row in &needs {
                list = match cards.get(&row.id) {
                    Some(card) => list.child(self.consent_row(row, card, a, cx)),
                    None => list.child(self.row(row, a, cx)),
                };
            }
        }
        if !done.is_empty() {
            list = list.child(self.done_heading(done.len(), a, cx));
            if self.done_open {
                for row in &done {
                    list = list.child(self.row(row, a, cx));
                }
            }
        }
        let header = div()
            .px(px(SPACE[2]))
            .pt(px(SPACE[2]))
            .pb(px(SPACE[0]))
            .child(label("Inbox", TypeRole::Title, Ink::Text, a));
        let note = self.note.as_ref().map(|n| {
            div()
                .px(px(SPACE[2]))
                .child(label(n, TypeRole::Meta, Ink::Needs, a))
        });
        card(a)
            .w(px(WIDTH))
            .max_h(px(MAX_HEIGHT))
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .shadow_lg()
            .child(header)
            .children(note)
            .child(scrollable(list))
            .part("Inbox Panel", a, cx)
    }
}

struct InboxRender {
    panel: Entity<InboxPanelView>,
}

impl Render for InboxRender {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        div()
            .size_full()
            .p(px(SIDE_PADDING))
            .fill(Ink::Window, a)
            .child(self.panel.clone())
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let done_open = shown(name)?;
    let consent = name.starts_with("inbox-consent");
    let fixture = match name {
        "inbox-consent" => consent_model(0),
        _ if consent => consent_model(1),
        _ => model(),
    };
    let feed = Arc::new(InboxFeed::fixed(fixture));
    let vault_off = name == "inbox-consent-vault-off";
    let panel = cx.new(|_| {
        let panel = InboxPanelView::fixed(feed, NOW_MS, done_open);
        if vault_off {
            panel.with_vault(false)
        } else {
            panel
        }
    });
    Some(cx.new(|_| InboxRender { panel }).into())
}
