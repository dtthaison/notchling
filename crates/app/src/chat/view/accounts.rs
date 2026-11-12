//! Chat's account parts (part U5): the header's limit meters and account button, the account menu
//! under it (this session's account, the targets it can move to, Move All Sessions To, Add
//! Account…, Codex Accounts…), and the Accounts and Vault sheets' openers. Moving a session is
//! Milestone 3's `session.move` (acting); the accounts side goes through the app's shared store.

use std::path::Path;
use std::time::Duration;

use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::{MENU_RADIUS, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button_with, icon, menu_note, menu_panel_wide, menu_rule, menu_section, KitStyled, MenuItem,
};
use notchling_kit::{Appearance, Icon, Ink};

use super::ChatView;
use crate::accounts::menu::{
    account_menu, move_all_account, move_needs_accounts, AccountMenu, Entry, Place, Target,
};
use crate::accounts::model::{CLAUDE_CODE, CODEX};
use crate::acting::{Control, Request};
use crate::chat::data::Tool;
use crate::mac::gate;
use crate::switches::Switch;

/// How often Chat reads the accounts store for the header, while the window is open.
pub(super) const POLL: Duration = Duration::from_secs(10);

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
            // The meters and the menu are outside the conversation, which needn't draw again; the
            // Sidebar does only when a row's account changed.
            this.update(cx, |view, cx| {
                view.redraw_chrome(cx);
                let rows = view.row_accounts();
                if rows != view.row_accounts {
                    view.row_accounts = rows;
                    view.redraw_sidebar(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The account each sidebar row names, from the accounts store now.
    pub(super) fn row_accounts(&self) -> Vec<Option<(String, bool)>> {
        let snapshot = self.accounts.snapshot();
        self.sessions
            .iter()
            .map(|s| {
                crate::chat::sidebar::row_account(s.tool, s.agent.config_dir.as_deref(), &snapshot)
                    .map(|a| (a.text, a.lane))
            })
            .collect()
    }

    /// The menu for the selected Claude Code session, if it has an account to show.
    pub(super) fn menu(&self) -> Option<AccountMenu> {
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

/// The account button under the title: the email with the lane, or the Codex login.
pub(super) fn account_control(
    view: &ChatView,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Stateful<Div>> {
    let session = view.selected_session()?;
    let tool = super::tool::of(session.tool);
    let super::tool::AccountButton { text, tip } = tool.account(view)?;
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
    Some(b.on_click(cx.listener(move |this, _, _, cx| tool.account_click(this, cx))))
}

/// A row of a menu with no checks (the kit's `MenuItem`).
pub(super) fn menu_item(id: impl Into<SharedString>, title: &str, a: Appearance) -> Stateful<Div> {
    MenuItem::new(id, title.to_owned()).build(a)
}

/// A row of a menu whose rows can be the current choice: the check at the right end.
pub(super) fn check_item(
    id: impl Into<SharedString>,
    title: &str,
    checked: bool,
    a: Appearance,
) -> Stateful<Div> {
    MenuItem::new(id, title.to_owned())
        .checked(checked)
        .build(a)
}

/// An item the `accounts` switch holds off: faint, no click, the reason on hover.
pub(super) fn held_item(
    id: impl Into<SharedString>,
    title: &str,
    control: Control,
    a: Appearance,
) -> Stateful<Div> {
    gate::tip(
        MenuItem::new(id, title.to_owned()).held(true).build(a),
        control,
        a,
    )
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
    let mut items = menu_panel_wide(a);
    let mut move_all = Vec::new();
    // Move All and a lane made for a saved account ask the daemon's accounts side.
    let accounts = view.switches.switches().control(Switch::Accounts);
    for (i, entry) in menu.entries.iter().enumerate() {
        match entry {
            Entry::Header(text) => {
                items = items.child(menu_section(text, a));
            }
            Entry::Target { index, current } => {
                let target = &menu.targets[*index];
                let id = SharedString::from(format!("account-target-{i}"));
                if !*current && move_needs_accounts(target) && !accounts.enabled {
                    items = items.child(held_item(id, &target.title, accounts, a));
                    continue;
                }
                let (entry, menu_clone) = (entry.clone(), menu.clone());
                items = items.child(check_item(id, &target.title, *current, a).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.menu_entry(entry.clone(), &menu_clone, cx)
                    }),
                ));
            }
            Entry::Note(text) => {
                items = items.child(menu_note(text, a));
            }
            Entry::Separator => {
                items = items.child(menu_rule(a));
            }
            Entry::MoveAll => {
                items = items.child(menu_section("Move All Sessions To", a));
                for (t, target) in menu.targets.iter().enumerate() {
                    let target = target.clone();
                    if !accounts.enabled {
                        let id = SharedString::from(format!("move-all-{t}"));
                        move_all.push(held_item(id, &target.title, accounts, a));
                        continue;
                    }
                    move_all.push(
                        menu_item(format!("move-all-{t}"), &target.title, a).on_click(
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
                items = items.child(menu_item(format!("account-entry-{i}"), title, a).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.menu_entry(entry.clone(), &menu_clone, cx)
                    }),
                ));
            }
        }
    }
    if let Some(off) = view.switches.switches().message(Switch::Accounts) {
        items = items.child(menu_note(off, a));
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
                    // Over the row under the reply box, right-aligned above its account button.
                    .bottom(px(MENU_BOTTOM))
                    .right(px(menu_right(view)))
                    .occlude()
                    .rounded(px(MENU_RADIUS))
                    .child(items),
            ),
    )
}

/// How far the account menu's bottom edge sits above the window's: over the row under the reply
/// box that holds the account button.
const MENU_BOTTOM: f32 = 48.0;

/// How far the account menu's right edge sits from the window's: past the Details Panel when it
/// shows (or the page panel in its place), the side padding and the limit ring beside the button.
fn menu_right(view: &ChatView) -> f32 {
    let details = if view.page.is_open() {
        view.page.shown_width()
    } else if view.details_hidden {
        0.0
    } else {
        super::inspector::WIDTH
    };
    details + SIDE_PADDING + 28.0
}
