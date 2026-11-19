use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, Context, Div, Entity, EventEmitter, Render, SharedString, Stateful, Window,
};
use notchling_engine::settings::Settings;
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon, icon_button, label, scrollable, window_appearance, KitPart,
    KitStyled, MenuItem,
};
use notchling_kit::{sentence, Appearance, Icon, IconSize, Ink, TypeRole};
use notchling_locale as t;
use serde_json::{Map, Value};

use super::agents::{Page, Tool};
use super::agents_view::agents_page;
use super::banner::Banners;
use super::client_model::{self, ClientPart};
use super::client_view::ClientHandlers;
use super::hook_rows::HooksShown;
use super::models_rows::ModelsShown;
use super::settings::{
    change, sections, shared_agent_sections, tool_sections, Daemon, Field, Row, Section,
};
use super::settings_skills_view::{skills_page, SkillsHandlers, SkillsPageState};
use super::skills_models;
use super::skills_rows::{reinstall_result, SkillsShown};
use super::view::{sheet_header, Acts, Dismiss};
use crate::accounts::view_rows::{segmented_full, Handler};
use crate::activity::view::ActivityView;
mod chrome;
mod cluster;
mod organizations;
mod shortcut;
mod sidecar;
use crate::mac::shell::{AppAccounts, AppActions, AppBridge, AppSwitchFeed};
use crate::switches::restart::{Area, LiveTurnOn};
use crate::switches::Switch;
use crate::welcome::hooks::Hooks;

const CLUSTER_AFTER: usize = 2;

pub struct SettingsView {
    store: Option<Settings>,
    values: Map<String, Value>,
    acts: Acts,
    error: Option<String>,
    busy: Option<Area>,
    hooks: HooksShown,
    skills: SkillsShown,
    models: ModelsShown,
    skills_busy: bool,
    skills_result: Option<(String, bool)>,
    client: Option<ClientPart>,
    watching: bool,
    confirming_back: bool,
    page: Page,
    tool: Tool,
    activity: Option<Entity<ActivityView>>,
    shortcut: shortcut::ShortcutState,
    cluster: Option<Arc<crate::cluster::feed::ClusterFeed>>,
    organizations: Option<Entity<crate::organizations::view::OrganizationsView>>,
    sidecar: sidecar::SidecarUi,
}

pub struct SettingChanged {
    pub key: String,
    pub value: Value,
}

impl EventEmitter<Dismiss> for SettingsView {}

impl EventEmitter<SettingChanged> for SettingsView {}

impl SettingsView {
    pub fn new(data_dir: Option<&Path>, acts: Acts) -> SettingsView {
        let store = data_dir.and_then(|d| Settings::open(&d.join("settings.json")).ok());
        let values = store
            .as_ref()
            .map(|s| s.values().clone())
            .unwrap_or_default();
        SettingsView {
            store,
            values,
            acts,
            error: None,
            busy: None,
            hooks: HooksShown::Checking,
            skills: SkillsShown::Checking,
            models: ModelsShown::Checking,
            skills_busy: false,
            skills_result: None,
            client: None,
            watching: false,
            confirming_back: false,
            page: Page::General,
            tool: Tool::ClaudeCode,
            activity: None,
            shortcut: shortcut::ShortcutState::default(),
            cluster: None,
            organizations: None,
            sidecar: sidecar::SidecarUi::default(),
        }
    }

    pub fn show_value(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        if self.values.get(key) != Some(&value) {
            self.values.insert(key.to_owned(), value);
            cx.notify();
        }
    }

    pub fn with_hooks(mut self, hooks: HooksShown) -> SettingsView {
        self.hooks = hooks;
        self
    }

    pub fn with_skills(mut self, skills: SkillsShown, models: ModelsShown) -> SettingsView {
        self.skills = skills;
        self.models = models;
        self
    }

    pub fn with_client(mut self, client: ClientPart) -> SettingsView {
        self.client = Some(client);
        self
    }

    pub fn with_page(mut self, page: Page) -> SettingsView {
        self.page = page;
        self
    }

    pub fn with_tool(mut self, tool: Tool) -> SettingsView {
        self.tool = tool;
        self
    }

    pub fn with_values(mut self, values: Map<String, Value>) -> SettingsView {
        self.values.extend(values);
        self
    }

    pub fn with_sidecar(mut self, shown: Option<super::sidecar_rows::Shown>) -> SettingsView {
        self.sidecar_shown(shown);
        self
    }

    pub fn with_activity(mut self, activity: Entity<ActivityView>) -> SettingsView {
        self.activity = Some(activity);
        self
    }

    pub fn show_agents(&mut self, cx: &mut Context<Self>) {
        self.show_page(Page::Agents, cx);
    }

    fn show_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        if page == Page::Agents {
            self.ask_client(cx);
        }
        if page == Page::Skills {
            self.ask_skills(cx);
        }
        if page == Page::Organizations {
            self.show_organizations(cx);
        }
        if page == Page::Activity {
            match &self.activity {
                Some(view) => view.update(cx, |view, cx| view.refresh(cx)),
                None => {
                    let actions = self.acts.actions.clone();
                    self.activity = Some(cx.new(|cx| ActivityView::live(actions, cx)));
                }
            }
        }
        cx.notify();
    }

    pub fn ask_client(&mut self, cx: &mut Context<Self>) {
        let actions = self.acts.actions.clone();
        let home = std::env::var("HOME").unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let part = cx
                .background_executor()
                .spawn(async move {
                    client_model::load(&actions, notchling_engine::versions::Tool::Claude, &home)
                })
                .await;
            this.update(cx, |view, cx| {
                view.client = Some(part);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn run_version_op(&mut self, op: &'static str, args: Value, cx: &mut Context<Self>) {
        let actions = self.acts.actions.clone();
        self.error = None;
        self.confirming_back = false;
        if let Some(part) = &mut self.client {
            part.busy = true;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let sent = cx
                .background_executor()
                .spawn(async move { actions.read(&crate::acting::Request { op, args }) })
                .await;
            this.update(cx, |view, cx| {
                if let Err(e) = sent {
                    view.error = Some(e);
                }
                view.watch_client(cx);
            })
            .ok();
        })
        .detach();
    }

    fn watch_client(&mut self, cx: &mut Context<Self>) {
        if self.watching {
            return;
        }
        self.watching = true;
        let actions = self.acts.actions.clone();
        let home = std::env::var("HOME").unwrap_or_default();
        cx.spawn(async move |this, cx| {
            for _ in 0..300 {
                let (actions, home) = (actions.clone(), home.clone());
                let part = cx
                    .background_executor()
                    .spawn(async move {
                        client_model::load(
                            &actions,
                            notchling_engine::versions::Tool::Claude,
                            &home,
                        )
                    })
                    .await;
                let busy = part.busy;
                let alive = this
                    .update(cx, |view, cx| {
                        view.client = Some(part);
                        cx.notify();
                    })
                    .is_ok();
                if !alive || !busy {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
            this.update(cx, |view, _| view.watching = false).ok();
        })
        .detach();
    }

    fn client_handlers(&self, cx: &mut Context<Self>) -> ClientHandlers {
        let tool = notchling_engine::versions::Tool::Claude.name();
        let go_back = self
            .client
            .as_ref()
            .and_then(|c| c.go_back.as_ref())
            .map(|g| g.to.clone());
        ClientHandlers {
            check: Rc::new(cx.listener(move |this, _, _, cx| {
                this.run_version_op(
                    notchling_local_api::ops::VERSIONS_CHECK,
                    serde_json::json!({ "tool": tool }),
                    cx,
                );
            })),
            update: Rc::new(cx.listener(move |this, _, _, cx| {
                this.run_version_op(
                    notchling_local_api::ops::VERSIONS_UPDATE_LOCAL,
                    serde_json::json!({ "tool": tool }),
                    cx,
                );
            })),
            ask_back: Rc::new(cx.listener(|this, _, _, cx| {
                this.confirming_back = true;
                cx.notify();
            })),
            confirm_back: Rc::new(cx.listener(move |this, _, _, cx| {
                if let Some(to) = go_back.clone() {
                    this.run_version_op(
                        notchling_local_api::ops::VERSIONS_UPDATE_LOCAL,
                        serde_json::json!({ "tool": tool, "to": to }),
                        cx,
                    );
                }
            })),
            cancel_back: Rc::new(cx.listener(|this, _, _, cx| {
                this.confirming_back = false;
                cx.notify();
            })),
            confirming: self.confirming_back,
        }
    }

    pub fn ask_skills(&mut self, cx: &mut Context<Self>) {
        let actions = self.acts.actions.clone();
        let home = std::env::var("HOME").unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let (skills, models) = cx
                .background_executor()
                .spawn(async move { skills_models::load(&actions, &home) })
                .await;
            this.update(cx, |view, cx| {
                view.skills = skills;
                view.models = models;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reinstall_skills(&mut self, cx: &mut Context<Self>) {
        let names = self.skills.names();
        if self.skills_busy || names.is_empty() {
            return;
        }
        self.skills_busy = true;
        self.skills_result = None;
        let actions = self.acts.actions.clone();
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { skills_models::reinstall(&actions, &names) })
                .await;
            this.update(cx, |view, cx| {
                view.skills_busy = false;
                view.skills_result = Some(match reinstall_result(reply) {
                    Ok(done) => (done.to_owned(), false),
                    Err(e) => (e, true),
                });
                view.ask_skills(cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn open_override(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.models.file().and_then(|f| f.path.clone()) else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        match notchling_engine::catalog::override_file::ensure(&path) {
            Ok(made) => {
                self.skills_result = None;
                cx.open_with_system(&path);
                if made {
                    self.ask_skills(cx);
                }
            }
            Err(why) => {
                self.skills_result = Some((t::settings::override_failed(&why), true));
            }
        }
        cx.notify();
    }

    fn skills_handlers(&self, cx: &mut Context<Self>) -> SkillsHandlers {
        SkillsHandlers {
            reinstall: Rc::new(cx.listener(|this, _, _, cx| this.reinstall_skills(cx))),
            open_override: Rc::new(cx.listener(|this, _, _, cx| this.open_override(cx))),
            reload: Rc::new(cx.listener(|this, _, _, cx| {
                this.skills_result = None;
                this.ask_skills(cx);
            })),
        }
    }

    pub fn ask_hooks(&mut self, hooks: Arc<Hooks>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let shown = cx
                .background_executor()
                .spawn(async move { HooksShown::from_plan(hooks.plan()) })
                .await;
            this.update(cx, |view, cx| {
                view.hooks = shown;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn daemon(&self, cx: &Context<Self>) -> Daemon {
        let mut daemon = Daemon::detached();
        daemon.acting = self.acts.acting();
        if let Some(bridge) = cx.try_global::<AppBridge>() {
            let snapshot = bridge.0.snapshot();
            daemon.bridge = snapshot.attached.then(|| snapshot.state.is_on());
        }
        if let Some(feed) = cx.try_global::<AppSwitchFeed>() {
            let s = feed.0.switches();
            if s.attached {
                daemon.accounts = Some(s.is_on(Switch::Accounts));
                daemon.vault = Some(s.is_on(Switch::Vault));
                daemon.install = Some(s.is_on(Switch::Install));
            }
        }
        daemon
    }

    fn set_switch(&mut self, area: Area, on: bool, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let (Some(actions), Some(bridge), Some(feed)) = (
            cx.try_global::<AppActions>().map(|g| g.0.clone()),
            cx.try_global::<AppBridge>().map(|g| g.0.clone()),
            cx.try_global::<AppSwitchFeed>().map(|g| g.0.clone()),
        ) else {
            return;
        };
        let live = LiveTurnOn {
            actions,
            bridge,
            feed,
        };
        self.busy = Some(area);
        self.error = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { live.set(area, on) })
                .await;
            this.update(cx, |view, cx| {
                view.busy = None;
                if let Err(e) = result {
                    view.error = Some(e);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn apply(&mut self, row: &Row, option: &str, cx: &mut Context<Self>) {
        let Some((key, value)) = change(row, option) else {
            return;
        };
        if let Some(store) = &mut self.store {
            if let Err(e) = store.set(key, value.clone()) {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        }
        self.error = None;
        if let Some(accounts) = cx.try_global::<AppAccounts>().map(|g| g.0.clone()) {
            let (key, sent) = (key.to_owned(), value.clone());
            cx.background_executor()
                .spawn(async move {
                    if let Err(e) = accounts.send_limit_setting(&key, &sent) {
                        eprintln!("settings: {key} didn't reach the daemon ({e})");
                    }
                })
                .detach();
        }
        self.values.insert(key.to_owned(), value.clone());
        if key == super::sidecar_rows::CHANNEL_KEY {
            self.ask_sidecar(cx);
        }
        cx.emit(SettingChanged {
            key: key.to_owned(),
            value,
        });
        cx.notify();
    }

    fn page_tabs(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .p(px(2.0))
            .gap(px(2.0))
            .rounded(px(RADIUS_SM))
            .fill(Ink::Raised, a)
            .part("Settings Pages", a, cx)
            .children(Page::ALL.iter().map(|&page| {
                let kind = if self.page == page {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Ghost
                };
                button(
                    SharedString::from(format!("settings-page-{}", page.id())),
                    page.title(),
                    kind,
                    PartState::Rest,
                    a,
                )
                .flex_1()
                .on_click(cx.listener(move |this, _, _, cx| this.show_page(page, cx)))
            }))
    }

    fn tool_tabs(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let options: Vec<(String, String)> = Tool::ALL
            .iter()
            .map(|t| (t.id().to_owned(), t.title().to_owned()))
            .collect();
        let pick: Handler<SettingsView, String> = Rc::new(|this, id, cx| {
            if let Some(tool) = Tool::from_id(&id) {
                this.tool = tool;
                cx.notify();
            }
        });
        let strip = segmented_full("settings-tool", &options, self.tool.id(), a, cx, pick);
        strip.part("Settings Agent Tabs", a, cx)
    }

    fn choice_list(
        &self,
        row: &Row,
        options: &[(&'static str, &'static str)],
        selected: &str,
        i: usize,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .pb(px(4.0))
            .children(options.iter().map(|(value, title)| {
                let r = row.clone();
                let value = (*value).to_owned();
                MenuItem::new(format!("setting-{i}-{value}"), *title)
                    .checked(selected == value)
                    .build(a)
                    .on_click(cx.listener(move |this, _, _, cx| this.apply(&r, &value, cx)))
            }))
    }

    fn section_view(
        &self,
        section: &Section,
        n: &mut usize,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut rows = card(a).flex().flex_col();
        for (k, row) in section.rows.iter().enumerate() {
            *n += 1;
            let note = match &row.field {
                Field::ReadOnly {
                    note: Some(note), ..
                } => Some(label(&sentence(note), TypeRole::Meta, Ink::TextFaint, a)),
                Field::Shortcut(_) => self.shortcut_note(a, cx),
                Field::List { options, selected } => {
                    Some(self.choice_list(row, options, selected, *n, a, cx))
                }
                _ => None,
            };
            rows = rows.child(
                div()
                    .px(px(14.0))
                    .py(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .when(k > 0, |r| {
                        r.border_t(px(1.0))
                            .border_color(hsla(Ink::Hairline.color(a)))
                    })
                    .child(
                        div()
                            .min_h(px(24.0))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(label(row.title, TypeRole::Body, Ink::Text, a).flex_1())
                            .child(self.field(row, *n, a, cx)),
                    )
                    .children(note),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(section.title, TypeRole::Section, Ink::TextFaint, a))
            .child(rows)
            .children(
                section
                    .footer
                    .map(|f| label(f, TypeRole::Meta, Ink::TextFaint, a)),
            )
    }

    fn field(&self, row: &Row, i: usize, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = |part: &str| SharedString::from(format!("setting-{i}-{part}"));
        match &row.field {
            Field::Toggle { on, .. } => {
                let r = row.clone();
                div().child(
                    switch(id("switch"), *on, a)
                        .on_click(cx.listener(move |this, _, _, cx| this.apply(&r, "", cx))),
                )
            }
            Field::Choice { options, selected } => div()
                .flex()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded(px(RADIUS_SM))
                .fill(Ink::Raised, a)
                .children(options.iter().map(|(value, title)| {
                    let r = row.clone();
                    let value = (*value).to_owned();
                    let kind = if *selected == value {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    };
                    button(id(&value), title, kind, PartState::Rest, a)
                        .on_click(cx.listener(move |this, _, _, cx| this.apply(&r, &value, cx)))
                })),
            Field::Stepper { value, .. } => {
                let (down, up) = (row.clone(), row.clone());
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        icon_button(
                            id("down"),
                            Icon::Minus,
                            IconSize::Button.points(),
                            ButtonKind::Secondary,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.apply(&down, "-1", cx))),
                    )
                    .child(label(&format!("{value}%"), TypeRole::Label, Ink::Text, a))
                    .child(
                        icon_button(
                            id("up"),
                            Icon::Plus,
                            IconSize::Button.points(),
                            ButtonKind::Secondary,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.apply(&up, "+1", cx))),
                    )
            }
            Field::Daemon { area, on } => {
                let (area, on) = (*area, *on);
                match on {
                    Some(on) if self.busy != Some(area) => {
                        div().child(switch(id("daemon"), on, a).on_click(
                            cx.listener(move |this, _, _, cx| this.set_switch(area, !on, cx)),
                        ))
                    }
                    Some(on) => div().opacity(0.5).child(switch(id("daemon"), on, a)),
                    None => div().opacity(0.4).child(switch(id("daemon"), false, a)),
                }
            }
            Field::Shortcut(setting) => self.shortcut_field(*setting, a, cx),
            Field::List { .. } => div(),
            Field::ReadOnly { value, .. } => div()
                .px(px(8.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .rounded_full()
                .fill(Ink::Raised, a)
                .child(label(value, TypeRole::Label, Ink::TextMuted, a)),
        }
    }
}

pub(super) fn switch(id: SharedString, on: bool, a: Appearance) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(32.0))
        .h(px(18.0))
        .p(px(2.0))
        .rounded_full()
        .flex()
        .when(on, |s| s.justify_end())
        .fill(if on { Ink::Accent } else { Ink::Raised }, a)
        .cursor_pointer()
        .child(div().size(px(14.0)).rounded_full().bg(gpui::white()))
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let values = self.values.clone();
        let read = move |key: &str| values.get(key).cloned();
        let daemon = self.daemon(cx);
        let done = button(
            "settings-done",
            t::settings::done(),
            ButtonKind::Primary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss)));
        let mut list = div()
            .id("settings-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(14.0));
        let mut n = 0;
        if self.page != Page::Activity {
            list = list.child(self.page_tabs(a, cx));
        }
        let general = if self.page == Page::General {
            sections(&read, &daemon)
        } else {
            Vec::new()
        };
        for (at, section) in general.iter().enumerate() {
            list = list.child(self.section_view(section, &mut n, a, cx));
            if at == CLUSTER_AFTER {
                list = list.children(self.cluster_section(a, cx));
            }
        }
        list = list.children(self.organizations_page());
        if self.page == Page::Agents {
            for section in shared_agent_sections(&read) {
                list = list.child(self.section_view(&section, &mut n, a, cx));
            }
            list = list.child(self.tool_tabs(a, cx));
            let mut settings = Vec::new();
            for section in tool_sections(self.tool, &read) {
                settings.push(self.section_view(&section, &mut n, a, cx));
                if self.tool == Tool::Antigravity {
                    settings.extend(self.sidecar_section(a, cx));
                }
            }
            let handlers = self.client_handlers(cx);
            list = list.child(agents_page(
                self.tool,
                settings,
                &self.hooks,
                self.client.as_ref(),
                &handlers,
                self.chrome_section(a, cx),
                a,
                cx,
            ));
        }
        if self.page == Page::Skills {
            let handlers = self.skills_handlers(cx);
            let state = SkillsPageState {
                skills: &self.skills,
                models: &self.models,
                busy: self.skills_busy,
                result: self.skills_result.as_ref(),
            };
            list = list.child(skills_page(&state, &handlers, a, cx));
        }
        let body = if self.page == Page::Activity {
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(SIDE_PADDING))
                        .pt(px(SIDE_PADDING))
                        .child(self.page_tabs(a, cx)),
                )
                .children(
                    self.activity
                        .clone()
                        .map(|view| div().flex_1().min_h(px(0.0)).child(view)),
                )
                .into_any_element()
        } else {
            scrollable(list).into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header(Icon::Settings, t::settings::title(), done, a))
            .child(body)
            .children(self.error.as_deref().map(|e| {
                label(e, TypeRole::Meta, Ink::Needs, a)
                    .px(px(SIDE_PADDING))
                    .pb(px(10.0))
            }))
    }
}

pub struct BannersView {
    pub banners: Banners,
}

impl Render for BannersView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let rows = self.banners.all().iter().enumerate().map(|(i, b)| {
            let dismiss = icon_button(
                SharedString::from(format!("banner-dismiss-{i}")),
                Icon::Close,
                IconSize::Button.points(),
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.banners.dismiss(i);
                cx.notify();
            }));
            let fill = match b.kind {
                super::banner::Kind::Error => Ink::Card,
                _ => Ink::AccentWash,
            };
            div()
                .w_full()
                .pl(px(14.0))
                .pr(px(6.0))
                .py(px(6.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(RADIUS_MD))
                .fill(fill, a)
                .border(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .child(
                    icon(b.icon(), TypeRole::Label.spec().size, b.ink(), a)
                        .w(px(16.0))
                        .flex_shrink_0()
                        .flex()
                        .justify_center(),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .children(
                            b.title
                                .as_deref()
                                .map(|t| label(t, TypeRole::Heading, Ink::Text, a)),
                        )
                        .child(label(&b.text, TypeRole::Label, Ink::Text, a)),
                )
                .child(dismiss)
        });
        div()
            .size_full()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .fill(Ink::Window, a)
            .children(rows)
    }
}
