//! Chat's account parts (part U5): the header's limit meters and account button, the account menu
//! under it (this session's account, the targets it can move to, Move All Sessions To, Add
//! Account…, Codex Accounts…), and the Accounts and Vault sheets' openers. Moving a session is
//! Milestone 3's `session.move` (acting); the accounts side goes through the app's shared store.

use std::path::Path;
use std::time::Duration;

use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::{RADIUS_MD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button_with, card, hsla, icon, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::ChatView;
use crate::accounts::limits::{self, Meter, Zone};
use crate::accounts::menu::{
    account_menu, move_all_account, move_needs_accounts, AccountMenu, Entry, Place, Target,
};
use crate::accounts::model::{CLAUDE_CODE, CODEX};
use crate::accounts::view_rows::meter;
use crate::acting::{Control, Request};
use crate::chat::data::Tool;
use crate::mac::gate;
use crate::switches::Switch;

/// How often Chat reads the accounts store for the header, while the window is open.
pub(super) const POLL: Duration = Duration::from_secs(10);

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Whether `codex` is on the PATH or in its usual places. It reads the disk, so Chat asks with the
/// accounts poll, not on each render.
pub(super) fn codex_installed(home: &Path) -> bool {
    notchling_engine::tools::codex::plan::find_binary(
        home,
        &std::env::var("PATH").unwrap_or_default(),
        Path::is_file,
    )
    .is_some()
}

impl ChatView {
    /// Reads the accounts store again off the UI thread (the header's meters and menu).
    pub(super) fn poll_accounts(&self, cx: &mut Context<Self>) {
        if !self.accounts.is_live() {
            return;
        }
        let (store, switches) = (self.accounts.clone(), self.switches.switches());
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move { store.refresh(&switches) })
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// The menu for the selected Claude Code session, if it has an account to show.
    fn menu(&self) -> Option<AccountMenu> {
        let session = self.selected_session()?;
        if session.tool != Tool::Claude {
            return None;
        }
        let agent = &session.agent;
        let movable = matches!(
            agent.target,
            notchling_engine::sessions::AgentTarget::Job(_)
        ) && agent.command.is_none()
            && agent.transcript_path.is_some();
        Some(account_menu(
            &self.accounts.snapshot(),
            agent.config_dir.as_deref(),
            movable,
            self.codex_installed,
        ))
    }

    pub(super) fn toggle_account_menu(&mut self, cx: &mut Context<Self>) {
        self.account_menu = !self.account_menu;
        cx.notify();
    }

    fn close_account_menu(&mut self, cx: &mut Context<Self>) {
        self.account_menu = false;
        cx.notify();
    }

    /// Moves the selected session to a target: a lane's folder, the main login's `~/.claude`, or a
    /// lane made first for a saved account (`lanes.create`), then `session.move`.
    fn move_to(&mut self, target: Target, cx: &mut Context<Self>) {
        self.account_menu = false;
        let Some(id) = self.selected.clone() else {
            return;
        };
        let acts = self.acts(cx);
        let (store, switches) = (self.accounts.clone(), self.switches.switches());
        let main_dir = self
            .root
            .home
            .join(".claude")
            .to_string_lossy()
            .into_owned();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let dir = match target.place {
                        Place::Main => main_dir,
                        Place::Lane(path) => path,
                        Place::Saved(account) => store.create_lane(&switches, &account)?.path,
                    };
                    acts.actions
                        .send(&Request::move_session(&id, &dir))
                        .map(|_| ())
                })
                .await;
            this.update(cx, |view, cx| {
                if let Err(e) = result {
                    view.tools.error = Some(format!("Couldn't move it: {e}"));
                }
                view.reload_now(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Move All Sessions To a target: the Accounts sheet shows the plan and asks.
    fn move_all_to(&mut self, target: Target, cx: &mut Context<Self>) {
        self.account_menu = false;
        let account = match move_all_account(&target) {
            Ok(account) => account,
            Err(why) => {
                self.tools.error = Some(why.to_owned());
                return cx.notify();
            }
        };
        let entity = self.accounts_sheet_entity(Some(CLAUDE_CODE), cx);
        entity.update(cx, |view, cx| {
            view.move_all(account, target.title.clone(), cx)
        });
        self.show_sheet(entity, cx);
    }

    fn menu_entry(&mut self, entry: Entry, menu: &AccountMenu, cx: &mut Context<Self>) {
        match entry {
            Entry::Target { index, current } => {
                if current {
                    return self.close_account_menu(cx);
                }
                if let Some(target) = menu.targets.get(index).cloned() {
                    self.move_to(target, cx);
                }
            }
            Entry::AddAccount => {
                self.account_menu = false;
                self.open_accounts_sheet(Some(CLAUDE_CODE), cx);
            }
            Entry::CodexAccounts => {
                self.account_menu = false;
                self.open_accounts_sheet(Some(CODEX), cx);
            }
            Entry::MoveAll | Entry::Header(_) | Entry::Note(_) | Entry::Separator => {}
        }
    }
}

/// The header's limit meters for the selected session: Claude Code's 5h and Week (the account in
/// use), Codex's bars (the active Codex login); nothing for Antigravity.
pub(super) fn header_meters(view: &ChatView, a: Appearance) -> Vec<Stateful<Div>> {
    let Some(session) = view.selected_session() else {
        return Vec::new();
    };
    let s = view.accounts.snapshot();
    let now = now_secs();
    let meters: Vec<Meter> = match session.tool {
        Tool::Claude if session.agent.config_dir.is_none() => s
            .limits
            .providers
            .iter()
            .find(|p| p.id == "claude")
            .map(|p| limits::claude_meters(&p.windows, now, Zone::LOCAL))
            .unwrap_or_default(),
        Tool::Codex => {
            let codex = s.providers.providers.iter().find(|p| p.id == CODEX);
            codex
                .and_then(|p| p.active_id.as_ref())
                .and_then(|id| s.usage.get(CODEX).and_then(|r| r.get(id)))
                .map(|report| limits::codex_meters(report, now, Zone::LOCAL))
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    meters
        .iter()
        .enumerate()
        .map(|(i, m)| meter(SharedString::from(format!("meter-{i}")), m, a))
        .collect()
}

/// The account button under the title: the email with the lane, or the Codex login.
pub(super) fn account_control(
    view: &ChatView,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Stateful<Div>> {
    let session = view.selected_session()?;
    let (text, tip): (String, String) = match session.tool {
        Tool::Claude => {
            let menu = view.menu()?;
            let text = match &menu.lane {
                Some(_) => format!("{} · lane", menu.label),
                None => menu.label.clone(),
            };
            (text, menu.tooltip)
        }
        Tool::Codex => {
            let s = view.accounts.snapshot();
            let codex = s.providers.providers.iter().find(|p| p.id == CODEX)?;
            let active = codex.active_id.as_ref()?;
            let email = codex
                .accounts
                .iter()
                .find(|acc| &acc.id == active)
                .map(|acc| acc.email.clone())?;
            (
                email,
                "The Codex account. Click to switch or add one.".to_owned(),
            )
        }
        Tool::Antigravity => return None,
    };
    let kind = if view.account_menu {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    // The email shrinks to "…" in a narrow header rather than spilling over what's beside it.
    // The chevron is placed over the text's own padding (see `marked_line`): wrapping the text in a
    // row would measure it narrow.
    let b = button_with(
        "account-button",
        div()
            .min_w(px(0.0))
            .relative()
            .pr(px(12.0))
            .child(text)
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(icon(Icon::ChevronDown, 7.0, Ink::Text, a)),
            )
            .elide(),
        kind,
        PartState::Rest,
        a,
    )
    .h(px(20.0))
    .px(px(6.0))
    .min_w(px(0.0))
    .flex_shrink();
    let b = gate::tip_text(b, tip.into(), a);
    Some(match session.tool {
        Tool::Claude => b.on_click(cx.listener(|this, _, _, cx| this.toggle_account_menu(cx))),
        _ => b.on_click(cx.listener(|this, _, _, cx| this.open_accounts_sheet(Some(CODEX), cx))),
    })
}

fn menu_item(id: SharedString, title: &str, checked: bool, a: Appearance) -> Stateful<Div> {
    menu_row(id, title, checked, Ink::Text, a)
        .cursor_pointer()
        .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
}

/// An item the `accounts` switch holds off: faint, no click, the reason on hover.
fn held_item(id: SharedString, title: &str, control: Control, a: Appearance) -> Stateful<Div> {
    gate::tip(menu_row(id, title, false, Ink::TextFaint, a), control, a)
}

fn menu_row(
    id: SharedString,
    title: &str,
    checked: bool,
    ink: Ink,
    a: Appearance,
) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(10.0))
        .py(px(5.0))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(8.0))
        // A fixed slot, so the titles line up whether or not one is ticked.
        .child(
            div()
                .w(px(14.0))
                .flex_shrink_0()
                .children(checked.then(|| icon(Icon::Check, TypeRole::Label.spec().size, ink, a))),
        )
        .child(label(title, TypeRole::Label, ink, a))
}

/// The account menu under the header's button, over a click-away layer.
pub(super) fn render_menu(
    view: &ChatView,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Div> {
    if !view.account_menu {
        return None;
    }
    let menu = view.menu()?;
    let mut items = card(a)
        .w(px(320.0))
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .shadow_lg();
    let mut move_all = Vec::new();
    // Move All and a lane made for a saved account ask the daemon's accounts side.
    let accounts = view.switches.switches().control(Switch::Accounts);
    for (i, entry) in menu.entries.iter().enumerate() {
        match entry {
            Entry::Header(text) => {
                items = items.child(
                    label(text, TypeRole::Section, Ink::TextFaint, a)
                        .px(px(10.0))
                        .pt(px(4.0)),
                );
            }
            Entry::Target { index, current } => {
                let target = &menu.targets[*index];
                let id = SharedString::from(format!("account-target-{i}"));
                if !*current && move_needs_accounts(target) && !accounts.enabled {
                    items = items.child(held_item(id, &target.title, accounts, a));
                    continue;
                }
                let (entry, menu_clone) = (entry.clone(), menu.clone());
                items = items.child(menu_item(id, &target.title, *current, a).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.menu_entry(entry.clone(), &menu_clone, cx)
                    }),
                ));
            }
            Entry::Note(text) => {
                items = items.child(
                    label(text, TypeRole::Meta, Ink::TextFaint, a)
                        .px(px(10.0))
                        .pb(px(4.0)),
                );
            }
            Entry::Separator => {
                items = items.child(div().h(px(1.0)).my(px(4.0)).fill(Ink::Hairline, a));
            }
            Entry::MoveAll => {
                items = items.child(
                    label("Move All Sessions To", TypeRole::Section, Ink::TextFaint, a)
                        .px(px(10.0))
                        .pt(px(4.0)),
                );
                for (t, target) in menu.targets.iter().enumerate() {
                    let target = target.clone();
                    if !accounts.enabled {
                        let id = SharedString::from(format!("move-all-{t}"));
                        move_all.push(held_item(id, &target.title, accounts, a));
                        continue;
                    }
                    move_all.push(
                        menu_item(
                            SharedString::from(format!("move-all-{t}")),
                            &target.title,
                            false,
                            a,
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.move_all_to(target.clone(), cx)),
                        ),
                    );
                }
                items = items.children(std::mem::take(&mut move_all));
            }
            Entry::AddAccount | Entry::CodexAccounts => {
                let title = if *entry == Entry::AddAccount {
                    "Add Account…"
                } else {
                    "Codex Accounts…"
                };
                let (entry, menu_clone) = (entry.clone(), menu.clone());
                items = items.child(
                    menu_item(
                        SharedString::from(format!("account-entry-{i}")),
                        title,
                        false,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu_entry(entry.clone(), &menu_clone, cx)
                    })),
                );
            }
        }
    }
    if let Some(off) = view.switches.switches().message(Switch::Accounts) {
        items = items.child(
            label(off, TypeRole::Meta, Ink::TextMuted, a)
                .px(px(10.0))
                .pt(px(4.0)),
        );
    }
    Some(
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("account-menu-click-away")
                    .absolute()
                    .size_full()
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| this.close_account_menu(cx))),
            )
            .child(
                div()
                    .absolute()
                    .top(px(60.0))
                    .left(px(notchling_kit::metric::SIDEBAR_WIDTH + SIDE_PADDING))
                    .occlude()
                    .rounded(px(RADIUS_MD))
                    .child(items),
            ),
    )
}
