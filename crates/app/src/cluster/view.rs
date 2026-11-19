use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyElement, AnyView, ClipboardItem, Context, Div, Entity, EventEmitter,
    Render, Subscription, Window,
};
use notchling_kit::metric::{CONTROL, RADIUS_SM, SIDE_PADDING, SPACE};
use notchling_kit::part::field_look;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, label, scrollable, window_appearance, KitPart, KitStyled, TextEditor, TextEditorEvent,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::add_view::AddUi;
use super::edit_addresses_view::{EditSheet, Switching};
use super::feed::ClusterFeed;
use super::from_tailscale_view::TailscaleUi;
use super::install::{applied_note, plan_of, Plan};
use super::model::ClusterModel;
use super::move_sheet::{MoveSheet, Names};
use super::promote::PromoteSheet;
use super::renders::{
    install_plan_reply, model_of, names, risk_reply, shown, Shown, RENDER_NOW, STANDBY,
    STANDBY_NAME, THIS_MAC,
};
use super::secrets::VaultAccess;
use super::secrets_view::{editor_body, editor_footer, editor_title, Editor, SecretsEvent};
use super::tool_updates_view::ToolUi;
use super::updates_view::UpdateUi;
use super::view_secrets::{secrets_fixture, Editing};
use super::wire::{self, Request};
use crate::accounts::flows::Alert;
use crate::accounts::view_rows::{alert_overlay, Handler};
use crate::sheets::header::sheet_header_closing;
use crate::sheets::view::Dismiss;
use crate::switches::feed::SwitchFeed;
use crate::switches::Switches;
use crate::vault::store::Vault;
use crate::welcome::facts::HookFacts;

const POLL: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InstallAsk {
    pub id: String,
    pub name: String,
    pub title: String,
    pub lines: Vec<String>,
}

pub struct ClusterSheetView {
    pub(super) feed: Arc<ClusterFeed>,
    pub(super) access: VaultAccess,
    pub(super) editing: Option<Editing>,
    pub(super) menu: Option<String>,
    pub(super) move_sheet: Option<MoveSheet>,
    pub(super) promote_sheet: Option<PromoteSheet>,
    pub(super) taking_back: bool,
    pub(super) continuing: Option<String>,
    pub(super) waited: BTreeSet<String>,
    pub(super) now: Option<i64>,
    pub(super) names: Names,
    pub(super) removing: Option<String>,
    pub(super) removing_machine: Option<String>,
    pub(super) switching: Option<Switching>,
    pub(super) edit: Option<EditSheet>,
    pub(super) tailscale: TailscaleUi,
    pub(super) tailnets: Vec<String>,
    pub(super) via_names: Vec<String>,
    pub(super) checking: BTreeSet<String>,
    pub(super) installing: Option<InstallAsk>,
    pub(super) add: AddUi,
    pub(super) update: UpdateUi,
    pub(super) tools: ToolUi,
    name: Entity<TextEditor>,
    token: Entity<TextEditor>,
    pub(super) busy: bool,
    pub(super) note: Option<(String, bool)>,
    drawn: u64,
    _name_events: Subscription,
    _token_events: Subscription,
}

impl EventEmitter<Dismiss> for ClusterSheetView {}

pub(crate) fn field_box(editor: &Entity<TextEditor>, focused: bool, a: Appearance) -> Div {
    div()
        .min_h(px(CONTROL))
        .px(px(SPACE[1] + 2.0))
        .flex()
        .items_center()
        .rounded(px(RADIUS_SM))
        .kit_type(TypeRole::Body)
        .look(&field_look(focused, a))
        .child(div().flex_1().min_w(px(0.0)).child(editor.clone()))
}

fn this_mac() -> String {
    notchling_engine::bridge::net::bridge::host_name()
        .map(|name| name.trim_end_matches(".local").to_owned())
        .unwrap_or_default()
}

impl ClusterSheetView {
    pub fn live(
        feed: Arc<ClusterFeed>,
        access: VaultAccess,
        cx: &mut Context<Self>,
    ) -> ClusterSheetView {
        let view = ClusterSheetView::build(feed, access, &this_mac(), cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let alive = this.update(cx, |view, cx| {
                let generation = view.feed.generation();
                if generation != view.drawn {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
        view.refresh(cx);
        view
    }

    pub fn with_names(mut self, names: Names) -> ClusterSheetView {
        self.names = names;
        self
    }

    pub fn fixed(
        feed: Arc<ClusterFeed>,
        access: VaultAccess,
        name: &str,
        cx: &mut Context<Self>,
    ) -> ClusterSheetView {
        ClusterSheetView::build(feed, access, name, cx)
    }

    fn build(
        feed: Arc<ClusterFeed>,
        access: VaultAccess,
        name: &str,
        cx: &mut Context<Self>,
    ) -> ClusterSheetView {
        let editor = cx.new(|cx| TextEditor::new(t::cluster::name_label(), 1, cx).single_line());
        editor.update(cx, |editor, cx| editor.set_text(name, cx));
        let events = cx.subscribe(&editor, |this, _, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Submit {
                this.create(cx);
            }
        });
        let token =
            cx.new(|cx| TextEditor::new(t::cluster::join_placeholder(), 1, cx).single_line());
        let token_events = cx.subscribe(&token, |this, _, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Submit {
                this.join(cx);
            }
        });
        let updates = feed.updates();
        let runs = feed.tool_runs();
        ClusterSheetView {
            feed,
            access,
            editing: None,
            menu: None,
            move_sheet: None,
            promote_sheet: None,
            taking_back: false,
            continuing: None,
            waited: BTreeSet::new(),
            now: None,
            names: Names::new(),
            removing: None,
            removing_machine: None,
            switching: None,
            edit: None,
            tailscale: TailscaleUi::default(),
            tailnets: Vec::new(),
            via_names: Vec::new(),
            checking: BTreeSet::new(),
            installing: None,
            add: AddUi::default(),
            update: UpdateUi::new(updates),
            tools: ToolUi::new(runs),
            name: editor,
            token,
            busy: false,
            note: None,
            drawn: 0,
            _name_events: events,
            _token_events: token_events,
        }
    }

    fn refresh(&self, cx: &mut Context<Self>) {
        if !self.feed.is_live() {
            return;
        }
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move { feed.refresh() })
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    pub(super) fn run(
        &mut self,
        request: Request,
        failed: fn(&str) -> String,
        cx: &mut Context<Self>,
    ) {
        if self.busy || !self.feed.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        self.menu = None;
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = feed.send(&request);
                    feed.refresh();
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                if let Err(error) = result {
                    view.note = Some((failed(&error), true));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).text().trim().to_owned();
        if name.is_empty() || !self.feed.snapshot().can_create() {
            return;
        }
        self.run(wire::create(&name), t::cluster::create_failed, cx);
    }

    fn join(&mut self, cx: &mut Context<Self>) {
        if self.busy || !self.feed.snapshot().can_join() {
            return;
        }
        let typed = self.token.read(cx).text().to_owned();
        let input = wire::join(&typed);
        if input.clears_field() {
            self.token.update(cx, |editor, cx| editor.set_text("", cx));
        }
        match input {
            wire::JoinInput::Nothing => {}
            wire::JoinInput::NotAToken => {
                self.note = Some((t::cluster::join_bad_token().to_owned(), true));
                cx.notify();
            }
            wire::JoinInput::Send(request) => {
                self.run(request, t::cluster::join_failed, cx);
            }
        }
    }

    fn turn_on(&mut self, cx: &mut Context<Self>) {
        self.run(wire::turn_on(), t::cluster::turn_on_failed, cx);
    }

    fn copy_token(&mut self, cx: &mut Context<Self>) {
        if self.busy || !self.feed.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { feed.token() })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.note = Some(match result {
                    Ok(token) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(token));
                        (t::cluster::token_copied().to_owned(), false)
                    }
                    Err(error) => (t::cluster::token_failed(&error), true),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn toggle_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        self.menu = if self.menu.as_deref() == Some(id) {
            None
        } else {
            Some(id.to_owned())
        };
        cx.notify();
    }

    pub(super) fn pick(
        &mut self,
        id: &str,
        ask: impl FnOnce(&ClusterModel, &str) -> Option<Request>,
        cx: &mut Context<Self>,
    ) {
        let request = ask(&self.feed.snapshot(), id);
        self.menu = None;
        match request {
            Some(request) => self.run(request, t::cluster::change_failed, cx),
            None => cx.notify(),
        }
    }

    pub(super) fn feed_offers_move(&self, id: &str) -> bool {
        let model = self.feed.snapshot();
        model.move_kind(id).is_some() && !(model.me() == id && model.offers_take_back())
    }

    pub(super) fn feed_offers_take_back(&self, id: &str) -> bool {
        let model = self.feed.snapshot();
        model.me() == id && model.offers_take_back()
    }

    pub(super) fn feed_offers_install(&self, id: &str) -> bool {
        self.feed.snapshot().install_plan(id).is_some()
    }

    fn name_of(&self, id: &str) -> String {
        self.feed
            .snapshot()
            .row(id)
            .map(|r| r.name.clone())
            .unwrap_or_default()
    }

    pub(super) fn install_hook(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(request) = self.feed.snapshot().install_plan(id) else {
            self.menu = None;
            cx.notify();
            return;
        };
        if self.busy || !self.feed.is_live() {
            return;
        }
        let (id, name) = (id.to_owned(), self.name_of(id));
        self.busy = true;
        self.note = None;
        self.menu = None;
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { asked(&feed, &request) })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                match result {
                    Err(error) => {
                        view.note = Some((t::cluster::install_failed(&name, &error), true));
                    }
                    Ok(facts) => match plan_of(&name, &facts) {
                        Plan::Confirm { title, lines } => {
                            view.installing = Some(InstallAsk {
                                id,
                                name,
                                title,
                                lines,
                            });
                        }
                        Plan::Say { text, problem } => view.note = Some((text, problem)),
                    },
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn install_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(ask) = self.installing.take() else {
            return;
        };
        let Some(request) = self.feed.snapshot().install_apply(&ask.id) else {
            cx.notify();
            return;
        };
        if self.busy || !self.feed.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = asked(&feed, &request);
                    feed.refresh();
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.note = Some(match result {
                    Ok(facts) => applied_note(&ask.name, &facts),
                    Err(error) => (t::cluster::install_failed(&ask.name, &error), true),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn install_alert(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        let ask = self.installing.as_ref()?;
        let alert = Alert {
            title: ask.title.clone(),
            message: ask.lines.join("\n"),
            button: t::cluster::install_confirm().to_owned(),
            destructive: false,
        };
        let on: Handler<Self, bool> = Rc::new(|this, yes, cx| {
            if yes {
                this.install_confirmed(cx);
            } else {
                this.installing = None;
                cx.notify();
            }
        });
        Some(alert_overlay(&alert, self.busy, a, cx, on))
    }

    fn connection(&self, model: &ClusterModel, a: Appearance, cx: &mut Context<Self>) -> Div {
        let line = model.line();
        let hint = model.off_hint();
        let ink = if line.is_problem() {
            Ink::Needs
        } else {
            Ink::TextMuted
        };
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .child(label(
                model.cluster_name().unwrap_or(""),
                TypeRole::Heading,
                Ink::Text,
                a,
            ))
            .child(div().flex_1().min_w(px(0.0)).child(label(
                &line.text(),
                TypeRole::Meta,
                ink,
                a,
            )));
        if model.can_turn_on() {
            row = row.child(
                button(
                    "cluster-turn-on",
                    t::cluster::turn_on(),
                    ButtonKind::Secondary,
                    self.state(),
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.turn_on(cx))),
            );
        }
        if model.can_copy_token() {
            row = row.child(
                button(
                    "cluster-copy-token",
                    t::cluster::copy_token(),
                    ButtonKind::Secondary,
                    self.state(),
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.copy_token(cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(SPACE[0]))
            .child(self.section_label(t::cluster::section_cluster(), a))
            .child(row)
            .children(hint.map(|hint| label(hint, TypeRole::Meta, Ink::TextMuted, a)))
    }

    fn section_label(&self, text: &str, a: Appearance) -> Div {
        div().child(label(text, TypeRole::Label, Ink::TextFaint, a))
    }

    fn members_block(
        &mut self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let tools = self.tool_view(model);
        let rows = tools.rows(model);
        let mut block = div().flex().flex_col().gap(px(SPACE[1]));
        if !rows.is_empty() {
            block = block.child(self.section_label(t::cluster::section_members(), a));
        }
        for row in rows {
            block = block.child(self.node_row(&row, &tools, a, cx));
        }
        block
    }

    fn state(&self) -> PartState {
        if self.busy {
            PartState::Disabled
        } else {
            PartState::Rest
        }
    }

    fn create_form(&self, focused: bool, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(label(t::cluster::none(), TypeRole::Body, Ink::TextMuted, a))
            .child(label(
                t::cluster::name_label(),
                TypeRole::Label,
                Ink::TextFaint,
                a,
            ))
            .child(field_box(&self.name, focused, a))
            .child(
                div().flex().child(
                    button(
                        "cluster-create",
                        t::cluster::create(),
                        ButtonKind::Primary,
                        self.state(),
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                ),
            )
    }

    fn join_form(&self, focused: bool, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(label(
                t::cluster::join_intro(),
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ))
            .child(label(
                t::cluster::join_label(),
                TypeRole::Label,
                Ink::TextFaint,
                a,
            ))
            .child(field_box(&self.token, focused, a))
            .child(
                div().flex().child(
                    button(
                        "cluster-join",
                        t::cluster::join(),
                        ButtonKind::Secondary,
                        self.state(),
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.join(cx))),
                ),
            )
            .part("Join Form", a, cx)
    }

    fn joining_body(&self, text: &str, a: Appearance) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(label(
                t::cluster::joining(),
                TypeRole::Heading,
                Ink::Text,
                a,
            ))
            .child(label(text, TypeRole::Body, Ink::TextMuted, a))
    }

    fn remove_alert(
        &self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let id = self.removing.as_ref()?;
        let name = model.row(id).map(|r| r.name.clone()).unwrap_or_default();
        let alert = Alert {
            title: t::cluster::remove_title().to_owned(),
            message: t::cluster::remove_note(&name),
            button: t::cluster::remove_confirm().to_owned(),
            destructive: true,
        };
        let on: Handler<Self, bool> = Rc::new(|this, yes, cx| {
            let Some(id) = this.removing.take() else {
                return;
            };
            if yes {
                this.pick(&id, ClusterModel::remove, cx);
            } else {
                cx.notify();
            }
        });
        Some(alert_overlay(&alert, self.busy, a, cx, on))
    }
}

impl Render for ClusterSheetView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        self.drawn = self.feed.generation();
        let model = self.feed.snapshot();
        let mut body = div()
            .id("cluster-body")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(SPACE[1] + SPACE[0] / 2.0));
        let section = |this: &Self, text: Div| {
            div()
                .flex()
                .flex_col()
                .gap(px(SPACE[0]))
                .child(this.section_label(t::cluster::section_cluster(), a))
                .child(text)
        };
        let mut machines = false;
        if (!self.feed.is_live() || self.feed.is_down()) && !model.is_loaded() {
            body = body.child(section(
                self,
                div().child(label(
                    t::cluster::detached(),
                    TypeRole::Body,
                    Ink::TextMuted,
                    a,
                )),
            ));
            machines = true;
        } else if !model.is_loaded() {
            body = body.child(label(
                t::cluster::loading(),
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ));
        } else if let Some(error) = model.unbuilt_error() {
            body = body.child(label(error, TypeRole::Body, Ink::Needs, a));
        } else if let Some(text) = model.joining_text() {
            body = body.child(self.joining_body(text, a));
        } else if model.can_create() {
            let name_focused = self.name.read(cx).is_focused(window);
            let token_focused = self.token.read(cx).is_focused(window);
            body = body
                .child(self.section_label(t::cluster::section_cluster(), a))
                .child(self.create_form(name_focused, a, cx))
                .child(self.join_form(token_focused, a, cx));
            machines = true;
        } else {
            body = body.child(self.connection(&model, a, cx));
            body = body.children(self.move_running_line(&model, a, cx));
            body = body.children(self.promote_running_line(&model, a, cx));
            body = body.children(self.add_rows(a, cx));
            body = body.children(self.update_banner_row(&model, a, cx));
            body = body.children(self.waiting_block(&model, a, cx));
            body = body.child(self.members_block(&model, a, cx));
            machines = true;
        }
        if machines {
            body = body.children(self.not_joined_block(&model, a, cx));
            if model.accepts_machines() {
                body = body.children(self.copies_block(&model, a, cx));
            }
        }
        if let Some((text, error)) = &self.note {
            body = body.child(label(
                text,
                TypeRole::Meta,
                if *error { Ink::Needs } else { Ink::TextMuted },
                a,
            ));
        }
        let can_add =
            machines && self.editing.is_none() && self.feed.add_source().parts().is_some();
        let trailing: Vec<AnyElement> = can_add
            .then(|| self.add_machine_button(a, cx).into_any_element())
            .into_iter()
            .collect();
        let alert = self.remove_alert(&model, a, cx);
        let machine_alert = self.machine_remove_alert(a, cx);
        let switch_alert = self.switch_alert(a, cx);
        let install = self.install_alert(a, cx);
        let take_back = self.take_back_alert(&model, a, cx);
        let continuing = self.continue_alert(&model, a, cx);
        let moving = self.move_overlay(&model, a, cx);
        let adding = self.add_overlay(window, a, cx);
        let editing_addresses = self.edit_overlay(window, a, cx);
        let from_tailscale = self.tailscale_overlay(window, a, cx);
        let updating = self.update_overlay(window, a, cx);
        let login = self.login_overlay(a, cx);
        let vault_new = self.vault_overlay();
        let on: Handler<Self, SecretsEvent> =
            Rc::new(|this, event, cx| this.secrets_event(event, cx));
        let editor = self.editing.as_ref().map(|e| Editor {
            name: &e.name,
            model: e.model.as_ref(),
            error: e.error.as_deref(),
            busy: self.busy,
        });
        let (title, content, footer) = match &editor {
            Some(editor) => (
                editor_title(editor.name),
                scrollable(
                    div()
                        .id("cluster-secrets-body")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .p(px(SIDE_PADDING))
                        .child(editor_body(editor, a, cx, &on)),
                )
                .into_any_element(),
                Some(editor_footer(editor, a, cx, &on)),
            ),
            None => (
                t::cluster::title().to_owned(),
                scrollable(body).into_any_element(),
                None,
            ),
        };
        div()
            .id("cluster-sheet")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .fill(Ink::Window, a)
            .overflow_hidden()
            .child(sheet_header_closing(
                Icon::Server,
                &title,
                trailing,
                a,
                cx,
                |this, _, cx| {
                    if this.editing.is_some() {
                        this.secrets_event(SecretsEvent::Cancel, cx);
                    } else {
                        cx.emit(Dismiss);
                    }
                },
            ))
            .child(content)
            .children(footer)
            .children(alert)
            .children(machine_alert)
            .children(switch_alert)
            .children(install)
            .children(take_back)
            .children(continuing)
            .children(moving)
            .children(adding)
            .children(editing_addresses)
            .children(from_tailscale)
            .children(updating)
            .children(login)
            .children(vault_new)
            .part("Cluster View", a, cx)
    }
}

fn asked(feed: &ClusterFeed, request: &Request) -> Result<HookFacts, String> {
    let reply = feed.send(request)?;
    HookFacts::parse(&reply).ok_or_else(|| t::cluster::install_unread().to_owned())
}

struct ClusterRender {
    sheet: Entity<ClusterSheetView>,
}

impl Render for ClusterRender {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        div()
            .size_full()
            .fill(Ink::Window, a)
            .child(self.sheet.clone())
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let which = shown(name)?;
    let feed = Arc::new(super::add::renders::feed_for(which, model_of(which)));
    let access = VaultAccess {
        local: Arc::new(Vault::fixed(Vec::new())),
        switches: Arc::new(SwitchFeed::fixed(Switches::all_on())),
    };
    let sheet = cx.new(|cx| {
        let mut sheet = ClusterSheetView::fixed(feed, access, THIS_MAC, cx);
        if which == Shown::Menu {
            sheet.menu = Some(STANDBY.to_owned());
        }
        sheet.now = Some(RENDER_NOW);
        if which == Shown::Move {
            sheet.move_sheet = Some(MoveSheet::running(STANDBY, "mac-mini"));
            sheet.names = names();
        }
        if which == Shown::Unfenced {
            sheet.move_sheet = Some(MoveSheet::running(STANDBY, STANDBY_NAME).with_from(THIS_MAC));
            sheet.names = names();
        }
        if which == Shown::TakeBack {
            sheet.taking_back = true;
            sheet.names = names();
        }
        if which == Shown::Promote {
            let mut ask = PromoteSheet::new(STANDBY, STANDBY_NAME, THIS_MAC);
            let model = sheet.feed.snapshot();
            ask.start(&model);
            ask.answered(Ok(risk_reply()));
            sheet.promote_sheet = Some(ask);
            sheet.names = names();
        }
        if which == Shown::Install || which == Shown::InstallOff {
            let on = which == Shown::Install;
            let facts = HookFacts::parse(&install_plan_reply(on)).unwrap_or_default();
            match plan_of("mac-mini", &facts) {
                Plan::Confirm { title, lines } => {
                    sheet.installing = Some(InstallAsk {
                        id: STANDBY.to_owned(),
                        name: "mac-mini".to_owned(),
                        title,
                        lines,
                    });
                }
                Plan::Say { text, problem } => sheet.note = Some((text, problem)),
            }
        }
        sheet.preview_add(which, cx);
        if which == Shown::Secrets {
            sheet.editing = Some(secrets_fixture(&sheet.feed.snapshot()));
        }
        sheet
    });
    Some(cx.new(|_| ClusterRender { sheet }).into())
}
