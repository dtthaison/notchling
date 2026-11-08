//! The Accounts window and Chat's Accounts sheet over GPUI (macOS): Accounts | Token Cost, a tab
//! per provider, the rows, the add panel and the alerts. Every call to the daemon runs on the
//! background executor through the store; a control the `accounts` switch doesn't allow is drawn
//! disabled with the daemon's off message on hover. A render has a fixed snapshot and switches and
//! never calls anything.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, ClipboardItem, Context, EventEmitter, Render, SharedString, Window,
};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::ui::{label, window_appearance, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::cost::Range;
use super::flows::{
    move_all_alert, move_all_banner, remove_alert, sign_in_page, Alert, Banner, NOT_A_SIGN_IN_PAGE,
};
use super::limits::Zone;
use super::model::{page, tabs, Page, Tab, CLAUDE_CODE};
use super::store::{Accounts, Snapshot};
use super::view_rows::{account_row, add_panel, alert_overlay, segmented, AddEvent, RowEvent};
use super::wire::{parse_move_result, Outcome};
use crate::sheets::view::Dismiss;
use crate::switches::feed::{Changes, SwitchFeed};
use crate::switches::Switches;

/// How often the window asks the daemon while it is open.
const REFRESH_EVERY: Duration = Duration::from_secs(5);

/// What an alert's confirm does.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    RemoveClaude(String),
    RemoveProvider(String, String),
    /// The daemon asked to confirm a switch: send it again with `confirm: true`.
    SwitchProvider(String, String),
    AddProvider(String),
    /// Move every session to an account (`None` is the main login's).
    MoveAll(Option<String>),
}

/// What a background job ended with.
enum Done {
    Nothing,
    Alert(Alert, Pending),
    Open(String),
    /// A Claude Code sign-in began: open its page and poll for its end.
    SignIn(String),
    /// How a Move All went, kept under the rows until the next action.
    Banner(Option<Banner>),
}

pub struct AccountsView {
    pub(super) store: Arc<Accounts>,
    feed: Arc<SwitchFeed>,
    pub(super) tab: Tab,
    provider: String,
    busy: bool,
    /// The row whose ⋯ menu is open.
    menu_open: Option<String>,
    alert: Option<(Alert, Pending)>,
    note: Option<String>,
    /// How the last Move All went (or that it's still going).
    banner: Option<Banner>,
    pub(super) range: Range,
    pub(super) account: String,
    /// In Chat's sheet: a Done button, and Dismiss on it.
    pub(super) in_sheet: bool,
    /// A render's clock and switches; the live ones otherwise.
    fixed: Option<(f64, Switches)>,
    seen: Changes,
}

impl EventEmitter<Dismiss> for AccountsView {}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl AccountsView {
    /// The live view: reads the daemon now, then every few seconds and whenever the feed says the
    /// accounts, providers or limits moved (dropping the view ends the loop).
    pub fn live(
        store: Arc<Accounts>,
        feed: Arc<SwitchFeed>,
        in_sheet: bool,
        cx: &mut Context<Self>,
    ) -> AccountsView {
        let view = AccountsView {
            seen: feed.changes(),
            store,
            feed,
            tab: Tab::Accounts,
            provider: CLAUDE_CODE.into(),
            busy: false,
            menu_open: None,
            alert: None,
            note: None,
            banner: None,
            range: Range::Days30,
            account: String::new(),
            in_sheet,
            fixed: None,
        };
        view.refresh(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                ticks = ticks.wrapping_add(1);
                let due = this.update(cx, |view, cx| {
                    let changes = view.feed.changes();
                    if changes.moves != view.seen.moves {
                        view.move_ended(cx);
                    }
                    let moved = changes != view.seen;
                    view.seen = changes;
                    moved || (ticks as u64).is_multiple_of(REFRESH_EVERY.as_secs())
                });
                match due {
                    Ok(true) => {
                        if this.update(cx, |view, cx| view.refresh(cx)).is_err() {
                            break;
                        }
                    }
                    Ok(false) => {}
                    Err(_) => break,
                }
            }
        })
        .detach();
        view
    }

    /// A render's: `store` and `feed` as given, on `provider`'s tab, at a fixed clock.
    pub fn fixed(
        store: Arc<Accounts>,
        feed: Arc<SwitchFeed>,
        provider: &str,
        tab: Tab,
        now: f64,
    ) -> AccountsView {
        AccountsView {
            seen: feed.changes(),
            fixed: Some((now, feed.switches())),
            store,
            feed,
            tab,
            provider: provider.into(),
            busy: false,
            menu_open: None,
            alert: None,
            note: None,
            banner: None,
            range: Range::Days30,
            account: String::new(),
            in_sheet: false,
        }
    }

    /// Opens on a provider's tab (Chat's Codex Accounts…).
    pub fn show_provider(&mut self, provider: &str, cx: &mut Context<Self>) {
        self.tab = Tab::Accounts;
        self.provider = provider.to_owned();
        cx.notify();
    }

    pub(super) fn switches(&self) -> Switches {
        match &self.fixed {
            Some((_, s)) => *s,
            None => self.feed.switches(),
        }
    }

    fn now(&self) -> f64 {
        self.fixed.map(|(now, _)| now).unwrap_or_else(now_secs)
    }

    pub(super) fn refresh(&self, cx: &mut Context<Self>) {
        if !self.store.is_live() {
            return;
        }
        let (store, switches, range) = (self.store.clone(), self.switches(), self.range);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    store.refresh(&switches);
                    store.refresh_cost(range.wire());
                })
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// Runs `job` off the UI thread, then reads the daemon again; an error is shown as the note.
    fn run(
        &mut self,
        job: impl FnOnce(&Accounts, &Switches) -> Result<Done, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy || !self.store.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        self.banner = None;
        self.menu_open = None;
        let (store, switches, range) = (self.store.clone(), self.switches(), self.range);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = job(&store, &switches);
                    store.refresh(&switches);
                    store.refresh_cost(range.wire());
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                match result {
                    Ok(Done::Nothing) => view.alert = None,
                    Ok(Done::Alert(alert, pending)) => view.alert = Some((alert, pending)),
                    // Only a known sign-in page opens: the link comes from the daemon.
                    Ok(Done::Open(url)) if sign_in_page(&url) => {
                        view.alert = None;
                        cx.open_url(&url);
                    }
                    Ok(Done::Open(_)) => {
                        view.alert = None;
                        view.note = Some(NOT_A_SIGN_IN_PAGE.into());
                    }
                    Ok(Done::SignIn(url)) => {
                        view.alert = None;
                        if sign_in_page(&url) {
                            cx.open_url(&url);
                        } else {
                            view.note = Some(NOT_A_SIGN_IN_PAGE.into());
                        }
                        view.poll_add(cx);
                    }
                    Ok(Done::Banner(banner)) => {
                        view.alert = None;
                        view.banner = banner;
                    }
                    Err(e) => {
                        view.alert = None;
                        view.note = Some(e);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn row_event(&mut self, provider: String, id: String, event: RowEvent, cx: &mut Context<Self>) {
        match event {
            RowEvent::ToggleMenu => {
                let key = format!("{provider}-{id}");
                self.menu_open = (self.menu_open.as_deref() != Some(&key)).then_some(key);
                cx.notify();
            }
            RowEvent::Remove => {
                self.menu_open = None;
                let s = self.store.snapshot();
                let (email, in_use) = super::view_tabs::row_identity(&s, &provider, &id);
                let pending = if provider == CLAUDE_CODE {
                    Pending::RemoveClaude(id)
                } else {
                    Pending::RemoveProvider(provider.clone(), id)
                };
                self.alert = Some((remove_alert(&provider, &email, in_use), pending));
                cx.notify();
            }
            RowEvent::Switch => {
                if provider == CLAUDE_CODE {
                    self.run(
                        move |s, sw| s.switch_claude(sw, &id).map(|_| Done::Nothing),
                        cx,
                    );
                } else {
                    self.run(
                        move |s, sw| match s.switch_provider(sw, &provider, &id, false)? {
                            Outcome::NeedsConfirmation {
                                title,
                                message,
                                button,
                            } => Ok(Done::Alert(
                                Alert {
                                    title,
                                    message,
                                    button,
                                    destructive: false,
                                },
                                Pending::SwitchProvider(provider, id),
                            )),
                            Outcome::Done { .. } => Ok(Done::Nothing),
                        },
                        cx,
                    );
                }
            }
        }
    }

    fn add_event(&mut self, event: AddEvent, cx: &mut Context<Self>) {
        let provider = self.provider.clone();
        let s = self.store.snapshot();
        match event {
            AddEvent::Add if provider == CLAUDE_CODE => self.run(
                |s, sw| {
                    s.clear_add();
                    s.begin_add_claude(sw).map(Done::SignIn)
                },
                cx,
            ),
            AddEvent::Add => self.run(
                move |s, sw| match s.add_provider(sw, &provider, false)? {
                    Outcome::NeedsConfirmation {
                        title,
                        message,
                        button,
                    } => Ok(Done::Alert(
                        Alert {
                            title,
                            message,
                            button,
                            destructive: false,
                        },
                        Pending::AddProvider(provider),
                    )),
                    Outcome::Done { sign_in_url, .. } => {
                        Ok(sign_in_url.map(Done::Open).unwrap_or(Done::Nothing))
                    }
                },
                cx,
            ),
            AddEvent::Open => {
                if let Some(url) = super::view_tabs::sign_in_url(&s, &provider) {
                    if sign_in_page(&url) {
                        cx.open_url(&url);
                    } else {
                        self.note = Some(NOT_A_SIGN_IN_PAGE.into());
                        cx.notify();
                    }
                }
            }
            AddEvent::CopyLink => {
                if let Some(url) = super::view_tabs::sign_in_url(&s, &provider) {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                    self.note = Some("Link copied.".into());
                    cx.notify();
                }
            }
            AddEvent::Cancel if provider == CLAUDE_CODE => self.run(
                |s, sw| s.poll_add_claude(sw, true).map(|_| Done::Nothing),
                cx,
            ),
            AddEvent::Cancel => self.run(
                move |s, sw| s.cancel_add_provider(sw, &provider).map(|_| Done::Nothing),
                cx,
            ),
            AddEvent::Dismiss => {
                self.store.clear_add();
                cx.notify();
            }
        }
    }

    fn alert_answered(&mut self, yes: bool, cx: &mut Context<Self>) {
        let Some((_, pending)) = self.alert.take() else {
            return;
        };
        cx.notify();
        if !yes {
            return;
        }
        match pending {
            Pending::RemoveClaude(id) => self.run(
                move |s, sw| s.remove_claude(sw, &id).map(|_| Done::Nothing),
                cx,
            ),
            Pending::RemoveProvider(p, id) => self.run(
                move |s, sw| s.remove_provider(sw, &p, &id).map(|_| Done::Nothing),
                cx,
            ),
            Pending::SwitchProvider(p, id) => self.run(
                move |s, sw| s.switch_provider(sw, &p, &id, true).map(|_| Done::Nothing),
                cx,
            ),
            Pending::AddProvider(p) => self.run(
                move |s, sw| match s.add_provider(sw, &p, true)? {
                    Outcome::Done { sign_in_url, .. } => {
                        Ok(sign_in_url.map(Done::Open).unwrap_or(Done::Nothing))
                    }
                    Outcome::NeedsConfirmation { .. } => Ok(Done::Nothing),
                },
                cx,
            ),
            Pending::MoveAll(target) => self.run(
                move |s, sw| {
                    s.move_all_now(sw, target.as_deref())
                        .map(|result| Done::Banner(move_all_banner(&result)))
                },
                cx,
            ),
        }
    }

    /// A daemon that answered a Move All before it ended said how it went in an event: replace
    /// the "still moving" line with it.
    fn move_ended(&mut self, cx: &mut Context<Self>) {
        let Some(end) = self.feed.last_move() else {
            return;
        };
        if let Some(banner) = move_all_banner(&parse_move_result(&end)) {
            self.banner = Some(banner);
            cx.notify();
        }
    }

    /// Move All Sessions To `target` (an account id; `None` is the main login's): the dry run's
    /// plan is shown first, and the move goes out on its confirm.
    pub fn move_all(&mut self, target: Option<String>, email: String, cx: &mut Context<Self>) {
        self.run(
            move |s, sw| {
                let plan = s.move_all_plan(sw, target.as_deref())?;
                Ok(Done::Alert(
                    move_all_alert(&email, &plan),
                    Pending::MoveAll(target),
                ))
            },
            cx,
        );
    }

    fn accounts_page(
        &self,
        s: &Snapshot,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let tabs_of = tabs(s);
        let options: Vec<(String, String)> = tabs_of
            .iter()
            .map(|t| (t.id.clone(), t.title.clone()))
            .collect();
        let pick: super::view_rows::Handler<Self, String> = Rc::new(|this, key: String, cx| {
            this.provider = key;
            this.menu_open = None;
            cx.notify();
        });
        let page: Page = page(
            &self.provider,
            s,
            &self.switches(),
            self.busy,
            self.now(),
            Zone::LOCAL,
        );
        let mut list = div()
            .id("accounts-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(10.0));
        if let Some(off) = page.off {
            list = list.child(label(off, TypeRole::Meta, Ink::TextMuted, a));
        }
        if let Some(empty) = page.empty {
            list = list.child(label(empty, TypeRole::Body, Ink::TextMuted, a).pt(px(20.0)));
        }
        if let Some(note) = &page.note {
            list = list.child(label(note, TypeRole::Meta, Ink::TextMuted, a));
        }
        for row in &page.rows {
            let key = format!("{}-{}", row.provider, row.id);
            let (provider, id) = (row.provider.clone(), row.id.clone());
            let on: super::view_rows::Handler<Self, RowEvent> = Rc::new(move |this, event, cx| {
                this.row_event(provider.clone(), id.clone(), event, cx)
            });
            list = list.child(account_row(
                row,
                self.menu_open.as_deref() == Some(&key),
                a,
                cx,
                on,
            ));
        }
        if let Some(b) = &self.banner {
            let ink = if b.warning {
                Ink::Needs
            } else {
                Ink::TextMuted
            };
            list = list.child(label(&b.text, TypeRole::Meta, ink, a));
        }
        if let Some(e) = page.error.as_deref().or(self.note.as_deref()) {
            list = list.child(label(e, TypeRole::Meta, Ink::Needs, a));
        }
        let on_add: super::view_rows::Handler<Self, AddEvent> =
            Rc::new(|this, event, cx| this.add_event(event, cx));
        div()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(SIDE_PADDING))
                    .pt(px(12.0))
                    .flex()
                    .justify_center()
                    .child(segmented(
                        "provider-tab",
                        &options,
                        &self.provider,
                        a,
                        cx,
                        pick,
                    )),
            )
            .child(list)
            .child(add_panel(&page.add, a, cx, on_add))
            .into_any_element()
    }
}

impl Render for AccountsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let s = self.store.snapshot();
        let body = match self.tab {
            Tab::Accounts => self.accounts_page(&s, a, cx),
            Tab::TokenCost => self.cost_tab(&s, a, cx).into_any_element(),
        };
        let alert = self.alert.as_ref().map(|(alert, _)| {
            let on: super::view_rows::Handler<Self, bool> =
                Rc::new(|this, yes, cx| this.alert_answered(yes, cx));
            alert_overlay(alert, self.busy, a, cx, on)
        });
        div()
            .id(SharedString::from("accounts-view"))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(self.header(a, cx))
            .child(body)
            .children(alert)
    }
}
