use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, Context, Div, FocusHandle, KeyDownEvent, Render, ScrollHandle,
    SharedString, Stateful, Window,
};
use notchling_engine::inbox::{ActionId, Item};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon_button_small, label, scrollable, window_appearance, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;
use serde_json::Value;

use super::change::{ChangeOpen, NoteEditor};
use super::consent_card::{vault_moved, ConsentCard, Env};
use super::consent_view::{buttons, card_body};
use super::feed::InboxFeed;
use super::model::{change_route, is_local, target, ChangeRoute, Row, Target};
use super::rows::head;
use super::run::{open_sign_in, run_of, Done, Run};
use super::sections::tab_empty;
use super::tab_reveal::{Reveal, Step};
use super::tray_rows::{deny_route, DenyRoute};
use super::triage::{
    blocking_cap, key_action, key_target, panel_max, KeyAction, RowErrors, Section, Triage,
};
use super::view_family::FamilyState;
use super::WIDTH;

pub use super::build::build_render;
use crate::phone::bridge::now_ms;

const POLL: Duration = Duration::from_millis(500);
const CLOCK_EVERY: u32 = 20;
const ALLOW_SETTLED_MS: i64 = 60_000;

type CloseFn = Rc<dyn Fn(&mut gpui::App)>;

pub struct InboxPanelView {
    pub(super) feed: Arc<InboxFeed>,
    fixed_now: Option<i64>,
    pub(super) tab: Section,
    sent_seen: HashSet<String>,
    pub(super) busy: HashSet<String>,
    pub(super) errors: RowErrors,
    pub(super) change: Option<ChangeOpen>,
    pub(super) editor: Option<NoteEditor>,
    pub(super) preset: Option<String>,
    on_close: Option<CloseFn>,
    handle: Option<FocusHandle>,
    escape_taken: bool,
    want_focus: bool,
    drawn: u64,
    shown: HashMap<String, Instant>,
    fixed_shown_ms: Option<i64>,
    vault_on: Option<bool>,
    vault_seen: Option<(u64, bool)>,
    cards_open: bool,
    allow_waiting: bool,
    pub(super) family: FamilyState,
    pub(super) focus: Option<String>,
    reveal: bool,
    scroll: ScrollHandle,
    blocking_scroll: ScrollHandle,
    pub(super) tabs_scroll: ScrollHandle,
    pub(super) tab_reveal: Option<Reveal>,
}

impl InboxPanelView {
    pub fn live(feed: Arc<InboxFeed>, on_close: CloseFn, cx: &mut Context<Self>) -> InboxPanelView {
        let mut view = InboxPanelView {
            feed,
            fixed_now: None,
            tab: Section::Waiting,
            sent_seen: HashSet::new(),
            busy: HashSet::new(),
            errors: RowErrors::default(),
            change: None,
            editor: None,
            preset: None,
            on_close: Some(on_close),
            handle: Some(cx.focus_handle()),
            escape_taken: false,
            want_focus: true,
            drawn: 0,
            shown: HashMap::new(),
            fixed_shown_ms: None,
            vault_on: None,
            vault_seen: None,
            cards_open: false,
            allow_waiting: false,
            family: FamilyState::default(),
            focus: None,
            reveal: false,
            scroll: ScrollHandle::new(),
            blocking_scroll: ScrollHandle::new(),
            tabs_scroll: ScrollHandle::new(),
            tab_reveal: None,
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

    pub fn fixed(feed: Arc<InboxFeed>, now_ms: i64, tab: Section) -> InboxPanelView {
        InboxPanelView {
            feed,
            fixed_now: Some(now_ms),
            tab,
            sent_seen: HashSet::new(),
            busy: HashSet::new(),
            errors: RowErrors::default(),
            change: None,
            editor: None,
            preset: None,
            on_close: None,
            handle: None,
            escape_taken: false,
            want_focus: false,
            drawn: 0,
            shown: HashMap::new(),
            fixed_shown_ms: Some(ALLOW_SETTLED_MS),
            vault_on: Some(true),
            vault_seen: None,
            cards_open: false,
            allow_waiting: false,
            family: FamilyState::default(),
            focus: None,
            reveal: false,
            scroll: ScrollHandle::new(),
            blocking_scroll: ScrollHandle::new(),
            tabs_scroll: ScrollHandle::new(),
            tab_reveal: Some(Reveal::new(tab.tab_index())),
        }
    }

    pub fn with_change(mut self, id: &str, route: ChangeRoute, note: &str) -> InboxPanelView {
        self.change = Some(ChangeOpen {
            id: id.to_owned(),
            route,
        });
        self.preset = Some(note.to_owned());
        self
    }

    pub fn with_error(mut self, id: &str, message: &str) -> InboxPanelView {
        let model = self.feed.snapshot(self.fixed_now.unwrap_or_default());
        self.errors.record(&model, id, message);
        self
    }

    pub(super) fn fail(&mut self, id: &str, message: String) {
        let model = self.feed.snapshot(0);
        self.errors.record(&model, id, &message);
    }

    pub(super) fn sync_errors(&mut self) {
        let model = self.feed.snapshot(0);
        self.errors.sync(&model);
    }

    pub(super) fn drop_stale_change(&mut self) {
        let Some(open) = &self.change else {
            return;
        };
        let model = self.feed.snapshot(0);
        if model.item(&open.id).and_then(change_route).is_none() {
            self.change = None;
        }
    }

    pub(super) fn select_tab(&mut self, tab: Section) {
        self.tab = tab;
        self.tab_reveal = Some(Reveal::new(tab.tab_index()));
        self.focus = None;
        let Some(open) = &self.change else {
            return;
        };
        let now = self.fixed_now.unwrap_or_else(now_ms);
        let triage = self.feed.snapshot(now).triage();
        let shown = triage
            .blocking
            .iter()
            .chain(triage.rows(tab))
            .any(|row| row.id == open.id);
        if !shown {
            self.change = None;
        }
    }

    pub(super) fn cancel_note(&mut self) -> bool {
        if self.change.take().is_none() {
            return false;
        }
        self.escape_taken = true;
        true
    }

    pub fn took_escape(&mut self) -> bool {
        std::mem::take(&mut self.escape_taken)
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(close) = self.on_close.clone() {
            cx.defer(move |cx| close(cx));
        }
    }

    pub fn focus_row(&mut self, id: &str, cx: &mut Context<Self>) {
        let now = self.fixed_now.unwrap_or_else(now_ms);
        let triage = self.feed.snapshot(now).triage();
        if let Some(section) = Section::TABS
            .into_iter()
            .find(|section| triage.rows(*section).iter().any(|row| row.id == id))
        {
            self.tab = section;
            self.tab_reveal = Some(Reveal::new(section.tab_index()));
        }
        self.focus = Some(id.to_owned());
        self.reveal = true;
        self.want_focus = true;
        cx.notify();
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
            self.sync_errors();
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

    fn bring_up(&mut self, id: &str, open: Option<&Value>, cx: &mut Context<Self>) {
        match open.and_then(target) {
            Some(Target::Approval(ticket)) | Some(Target::Request(ticket)) => {
                crate::mac::vault_prompts::raise(&ticket);
            }
            Some(Target::Settings { page, .. }) if page == "agents" => {
                cx.defer(crate::mac::shell::open_chat_settings_agents);
            }
            Some(Target::Machines(_)) => {
                self.fail(id, t::inbox::machines_hint().to_owned());
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
            self.bring_up(&id, open.as_ref(), cx);
            return;
        }
        if !self.busy.insert(id.clone()) {
            return;
        }
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
                    view.fail(&id, e);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn consent_act(
        &mut self,
        row_id: String,
        id: String,
        allow: bool,
        app_applied: bool,
        run: Run,
        cx: &mut Context<Self>,
    ) {
        if !self.busy.insert(id.clone()) {
            return;
        }
        let feed = self.feed.clone();
        let sent = id.clone();
        let open_form = allow && app_applied && matches!(run, Run::Plain);
        let run = if allow { run } else { Run::Plain };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match &run {
                        Run::Switch(switch) => {
                            feed.allow_switch(&sent, switch).map(|_| Done::Plain)
                        }
                        Run::Cluster(apply) => {
                            feed.allow_cluster(&sent, apply).map(|_| Done::Plain)
                        }
                        Run::Account(apply) => feed.allow_account(&sent, apply).map(Done::Sign),
                        Run::Family(apply, parts, admin) => feed
                            .allow_family(&sent, apply, parts, admin.as_ref())
                            .map(Done::Note),
                        Run::Plain if open_form => feed.consent_opened(&sent).map(|_| Done::Form),
                        Run::Plain => feed.consent_answer(&sent, allow).map(|_| Done::Plain),
                    }
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy.remove(&id);
                view.family.forget(&id, cx);
                match result {
                    Ok(Done::Note(Some(note))) => view.fail(&row_id, note),
                    Ok(Done::Note(None)) => {}
                    Ok(Done::Sign(sign)) => open_sign_in(sign, cx),
                    Ok(Done::Form) => {
                        if let Some(feed) = cx.try_global::<crate::mac::shell::AppSwitchFeed>() {
                            feed.0.allow_request(&id);
                        }
                        crate::mac::vault_prompts::raise(&id);
                    }
                    Ok(_) => {}
                    Err(e) => view.fail(&row_id, e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
        if let Some(change) = self.change_button(row, live, a, cx) {
            buttons = buttons.child(change);
        }
        buttons
    }

    fn change_button(
        &self,
        row: &Row,
        live: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let route = row.change.clone()?;
        let state = if live {
            PartState::Rest
        } else {
            PartState::Disabled
        };
        let id = SharedString::from(format!("inbox-change-{}", row.id));
        let mut one = button(id, t::consent::change(), ButtonKind::Secondary, state, a);
        if live {
            let row_id = row.id.clone();
            one = one.on_click(cx.listener(move |this, _, window, cx| {
                this.open_change(row_id.clone(), route.clone(), window, cx);
            }));
        }
        Some(one)
    }

    fn error_line(&self, row_id: &str, a: Appearance) -> Option<Div> {
        self.errors
            .of(row_id)
            .map(|message| div().child(label(message, TypeRole::Meta, Ink::Needs, a)))
    }

    fn body(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut body = div()
            .id(SharedString::from(format!("inbox-body-{}", row.id)))
            .flex()
            .flex_col()
            .gap(px(SPACE[0] / 2.0))
            .child(head(row, row.enabled, a))
            .children(super::rows::chips(row, a))
            .children((!row.detail.is_empty()).then(|| {
                label(&row.detail, TypeRole::Meta, Ink::TextMuted, a)
                    .w_full()
                    .elide()
            }));
        if row.enabled && row.open.as_ref().and_then(target).is_some() {
            let (open, id) = (row.open.clone(), row.id.clone());
            body = body
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.bring_up(&id, open.as_ref(), cx);
                    cx.notify();
                }));
        }
        body
    }

    fn row(&self, row: &Row, window: &Window, a: Appearance, cx: &mut Context<Self>) -> Div {
        let buttons = if row.enabled && !row.actions.is_empty() {
            Some(self.action_buttons(row, a, cx))
        } else if row.section == Section::DidntWork {
            Some(div().flex().child(self.clear_button(&row.id, a, cx)))
        } else {
            None
        };
        div()
            .px(px(SPACE[2]))
            .py(px(SPACE[1] + SPACE[0] / 2.0))
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(self.body(row, a, cx))
            .children(self.error_line(&row.id, a))
            .children(buttons)
            .children(self.change_field(&row.id, window, a, cx))
            .part("Inbox Row", a, cx)
    }

    fn consent_row(
        &mut self,
        row: &Row,
        card: &ConsentCard,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = self.busy.contains(&card.id);
        let bar = buttons(card, busy, a);
        let mut allow = bar.allow;
        let mut deny = bar.deny;
        let row_id = row.id.clone();
        if card.allow.enabled() && !busy {
            let (id, app_applied, run) = (card.id.clone(), card.app_applied, run_of(card));
            let row_id = row_id.clone();
            allow = allow.on_click(cx.listener(move |this, _, _, cx| {
                let run = this.family.fill(run.clone(), &id, cx);
                this.consent_act(row_id.clone(), id.clone(), true, app_applied, run, cx);
            }));
        }
        if card.answerable && !busy {
            let (id, app_applied) = (card.id.clone(), card.app_applied);
            let row_id = row_id.clone();
            deny = deny.on_click(cx.listener(move |this, _, _, cx| {
                this.consent_act(
                    row_id.clone(),
                    id.clone(),
                    false,
                    app_applied,
                    Run::Plain,
                    cx,
                );
            }));
        }
        let change = self.change_button(row, card.answerable && !busy, a, cx);
        div()
            .px(px(SPACE[2]))
            .py(px(SPACE[1] + SPACE[0] / 2.0))
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(card_body(row, card, a))
            .children(self.family_block(card, window, a, cx))
            .children(self.error_line(&row.id, a))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(SPACE[1]))
                    .child(allow)
                    .child(deny)
                    .children(change),
            )
            .children(self.change_field(&row.id, window, a, cx))
            .part("Consent Card", a, cx)
    }

    fn cards(
        &mut self,
        snapshot: &super::model::InboxModel,
        rows: &[&Row],
        now: i64,
        cx: &mut Context<Self>,
    ) -> HashMap<String, ConsentCard> {
        let vault_on = self.vault_on.unwrap_or_else(|| {
            cx.try_global::<crate::mac::shell::AppSwitchFeed>()
                .is_some_and(|f| f.0.switches().is_on(crate::switches::Switch::Vault))
        });
        let mut out = HashMap::new();
        for row in rows.iter().copied().filter(|r| r.enabled) {
            let Some(id) = row.open.as_ref().and_then(super::model::consent_of) else {
                continue;
            };
            let Some(view) = snapshot.consent(id) else {
                continue;
            };
            self.family_prepare(view, cx);
            let first = *self.shown.entry(id.to_owned()).or_insert_with(Instant::now);
            let shown_ms = self
                .fixed_shown_ms
                .unwrap_or_else(|| first.elapsed().as_millis() as i64);
            let env = Env {
                now_ms: now,
                vault_on,
                shown_ms,
                node: row.node.clone(),
                unticked: self.family.unticked(id),
                admin_ready: self.family.admin_ready(id, cx),
                place: snapshot.item(&row.id).and_then(|item| item.place.clone()),
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

    fn nav_ids(&self, triage: &Triage) -> Vec<String> {
        triage
            .blocking
            .iter()
            .chain(triage.rows(self.tab))
            .map(|row| row.id.clone())
            .collect()
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let now = self.fixed_now.unwrap_or_else(now_ms);
        let ids = self.nav_ids(&self.feed.snapshot(now).triage());
        if ids.is_empty() {
            return;
        }
        let at = self
            .focus
            .as_ref()
            .and_then(|id| ids.iter().position(|held| held == id));
        let next = match (at, by) {
            (None, 1) => 0,
            (None, _) => ids.len() - 1,
            (Some(at), by) => (at as isize + by).rem_euclid(ids.len() as isize) as usize,
        };
        self.focus = Some(ids[next].clone());
        self.reveal = true;
        cx.notify();
    }

    fn focused_target(&self) -> Option<Item> {
        let id = self.focus.clone()?;
        let now = self.fixed_now.unwrap_or_else(now_ms);
        let model = self.feed.snapshot(now);
        key_target(&model.triage(), self.tab, &id)?;
        model.item(&id).cloned()
    }

    fn open_focused(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.focused_target() else {
            return;
        };
        self.bring_up(&item.id, item.open.as_ref(), cx);
    }

    fn deny_focused(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.focused_target() else {
            return;
        };
        let id = item.id.clone();
        match deny_route(&item) {
            Some(DenyRoute::Consent(consent)) => {
                self.consent_act(id, consent, false, false, Run::Plain, cx)
            }
            Some(DenyRoute::Answer(action)) => self.act(id, action, None, cx),
            None => {}
        }
    }

    fn key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        let focused = self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.is_focused(window));
        let action = key_action(
            event.keystroke.key.as_str(),
            event.keystroke.modifiers.platform,
            self.change.is_some(),
            focused,
        );
        match action {
            KeyAction::None => {}
            KeyAction::CancelNote => {
                self.cancel_note();
                cx.notify();
            }
            KeyAction::Step(by) => self.step(by, cx),
            KeyAction::Open => self.open_focused(cx),
            KeyAction::Deny => self.deny_focused(cx),
        }
    }
}

impl Render for InboxPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        self.drawn = self.feed.generation();
        let now = self.fixed_now.unwrap_or_else(now_ms);
        let snapshot = self.feed.snapshot(now);
        let triage = snapshot.triage();
        self.errors.sync(&snapshot);
        self.drop_stale_change();
        if std::mem::take(&mut self.want_focus) && self.change.is_none() {
            if let Some(handle) = &self.handle {
                handle.focus(window);
            }
        }
        self.prepare_change(cx);
        let shown_rows: Vec<&Row> = triage
            .blocking
            .iter()
            .chain(triage.rows(self.tab))
            .collect();
        let cards = self.cards(&snapshot, &shown_rows, now, cx);
        let panel_max = panel_max(f32::from(window.viewport_size().height));
        let mut pinned = div()
            .id("inbox-blocking")
            .max_h(px(blocking_cap(panel_max)))
            .flex_shrink_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let mut list = div()
            .id("inbox-list")
            .min_h(px(0.0))
            .pb(px(SPACE[1]))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let mut pinned_count = 1usize;
        let mut count = 1usize;
        if triage.blocking.is_empty() {
            pinned = pinned.child(self.calm_block(&triage, a));
        } else {
            pinned = pinned.child(self.blocking_heading(triage.blocking.len(), a));
        }
        let place = |this: &mut Self,
                     list: Stateful<Div>,
                     row: &Row,
                     count: &mut usize,
                     pinned: bool,
                     cx: &mut Context<Self>|
         -> Stateful<Div> {
            let mut shown = match cards.get(&row.id) {
                Some(card) => this.consent_row(row, card, window, a, cx),
                None => this.row(row, window, a, cx),
            };
            if this.focus.as_deref() == Some(row.id.as_str()) {
                shown = shown.bg(hsla(Ink::AccentWash.color(a)));
                if std::mem::take(&mut this.reveal) {
                    let handle = if pinned {
                        &this.blocking_scroll
                    } else {
                        &this.scroll
                    };
                    handle.scroll_to_item(*count);
                }
            }
            *count += 1;
            list.child(shown)
        };
        for row in &triage.blocking {
            pinned = place(self, pinned, row, &mut pinned_count, true, cx);
        }
        if let Some(mut reveal) = self.tab_reveal.take() {
            let ready = self.tabs_scroll.bounds_for_item(reveal.index).is_some();
            match reveal.step(ready) {
                Step::Apply => self.tabs_scroll.scroll_to_item(reveal.index),
                Step::Retry => {
                    self.tab_reveal = Some(reveal);
                    window.request_animation_frame();
                }
                Step::Drop => {}
            }
        }
        list = list.child(self.tab_bar(&triage, a, cx));
        let rows = triage.rows(self.tab);
        if rows.is_empty() {
            list = list.child(self.empty_line(tab_empty(self.tab), a));
            count += 1;
        }
        for row in rows {
            list = place(self, list, row, &mut count, false, cx);
        }
        let close = icon_button_small(
            "inbox-close",
            Icon::Close,
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.close(cx)));
        let header = div()
            .px(px(SPACE[2]))
            .pt(px(SPACE[2]))
            .pb(px(SPACE[0]))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .child(label(t::inbox::title(), TypeRole::Title, Ink::Text, a)),
            )
            .child(close);
        let warning = super::warning::shown().then(|| {
            div().px(px(SPACE[2])).child(label(
                t::inbox::no_channel(),
                TypeRole::Meta,
                Ink::Needs,
                a,
            ))
        });
        let mut panel = card(a)
            .w(px(WIDTH))
            .max_h(px(panel_max))
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .shadow_lg();
        if let Some(handle) = &self.handle {
            panel = panel.track_focus(handle).on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| this.key(event, window, cx)),
            );
        }
        panel
            .child(header)
            .children(warning)
            .child(scrollable(pinned).tracking(&self.blocking_scroll))
            .child(scrollable(list).tracking(&self.scroll))
            .child(self.keys_hint(a))
            .part("Inbox Panel", a, cx)
    }
}
