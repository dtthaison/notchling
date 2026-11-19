use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Div, EventEmitter, Render, Window};
use notchling_kit::part::ButtonKind;
use notchling_kit::ui::{card, label, window_appearance, KitPart};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;
use serde_json::Value;

use super::cache::Cache;
use super::check::{CheckModel, Resolve};
use super::picks::Options;
use super::profile::{Profile, CUSTOM, TOOLS};
use super::wire;
use crate::accounts::view_rows::{segmented_full, Handler};
use crate::acting::Request;
use crate::actions::Actions;
use crate::sheets::client_view::{action, Click};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Roles,
    Check,
    Cache,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Roles, Tab::Check, Tab::Cache];

    pub fn key(self) -> &'static str {
        match self {
            Tab::Roles => "roles",
            Tab::Check => "check",
            Tab::Cache => "cache",
        }
    }

    pub fn from_key(key: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.key() == key)
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Roles => t::costs::tab_roles(),
            Tab::Check => t::costs::tab_check(),
            Tab::Cache => t::costs::tab_cache(),
        }
    }
}

pub struct OpenSetup;

pub struct CostsView {
    pub(super) actions: Option<Arc<Actions>>,
    pub(super) claude_dir: Option<PathBuf>,
    pub(super) home: String,
    pub(super) profile: Option<Profile>,
    pub(super) options: Options,
    pub(super) load_error: Option<String>,
    pub(super) tab: Tab,
    pub(super) picker: Option<(String, usize)>,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
    pub(super) check: CheckModel,
    pub(super) cache: Cache,
}

impl EventEmitter<OpenSetup> for CostsView {}

impl CostsView {
    pub fn live(
        actions: Arc<Actions>,
        claude_dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> CostsView {
        let mut view = CostsView {
            actions: Some(actions),
            claude_dir,
            home: std::env::var("HOME").unwrap_or_default(),
            profile: None,
            options: Options::default(),
            load_error: None,
            tab: Tab::Roles,
            picker: None,
            busy: false,
            error: None,
            check: CheckModel::default(),
            cache: Cache::default(),
        };
        view.refresh(cx);
        view
    }

    pub fn fixed(
        profile: Profile,
        options: Options,
        tab: Tab,
        picker: Option<(String, usize)>,
        check: CheckModel,
        cache: Cache,
    ) -> CostsView {
        CostsView {
            actions: None,
            claude_dir: None,
            home: super::renders::HOME.to_owned(),
            profile: Some(profile),
            options,
            load_error: None,
            tab,
            picker,
            busy: false,
            error: None,
            check,
            cache,
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let profile = actions.read_full(&wire::profile()).map_err(|f| f.message);
                    let catalog = |tool: &str| actions.read(&wire::catalog(tool)).ok();
                    let lists = [catalog("claude"), catalog("codex"), catalog("antigravity")];
                    (profile, lists)
                })
                .await;
            this.update(cx, |view, cx| view.landed(loaded.0, &loaded.1, cx))
                .ok();
        })
        .detach();
        self.load_tab(cx);
    }

    fn landed(
        &mut self,
        reply: Result<Value, String>,
        lists: &[Option<Value>; 3],
        cx: &mut Context<Self>,
    ) {
        match reply.map(|r| Profile::from_reply(&r)) {
            Ok(Some(profile)) => {
                self.options =
                    Options::from_replies(lists[0].as_ref(), lists[1].as_ref(), lists[2].as_ref())
                        .or_known(&profile);
                self.profile = Some(profile);
                self.load_error = None;
            }
            Ok(None) => self.load_error = Some(t::costs::unreadable().to_owned()),
            Err(why) => self.load_error = Some(why),
        }
        cx.notify();
    }

    fn call(&mut self, request: Request, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { actions.read_full(&request).map_err(|f| f.message) })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                match reply.map(|r| Profile::from_reply(&r)) {
                    Ok(Some(profile)) => view.profile = Some(profile),
                    Ok(None) => view.error = Some(t::costs::unreadable().to_owned()),
                    Err(why) => view.error = Some(why),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn set_preset(&mut self, key: &str, cx: &mut Context<Self>) {
        if key == CUSTOM {
            return;
        }
        self.picker = None;
        self.call(wire::preset(key), cx);
    }

    pub(super) fn set_pick(&mut self, role: &str, tool: usize, text: &str, cx: &mut Context<Self>) {
        self.call(wire::pick(role, TOOLS[tool], text), cx);
    }

    pub(super) fn clear_pick(&mut self, role: &str, tool: usize, cx: &mut Context<Self>) {
        self.call(wire::clear_pick(role, TOOLS[tool]), cx);
    }

    pub(super) fn open_picker(&mut self, role: &str, tool: usize, cx: &mut Context<Self>) {
        let same = self
            .picker
            .as_ref()
            .is_some_and(|(r, t)| r == role && *t == tool);
        self.picker = (!same).then(|| (role.to_owned(), tool));
        cx.notify();
    }

    pub(super) fn show_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.load_tab(cx);
        cx.notify();
    }

    fn load_tab(&mut self, cx: &mut Context<Self>) {
        match self.tab {
            Tab::Roles => {}
            Tab::Check => self.load_check(None, cx),
            Tab::Cache => self.load_cache(cx),
        }
    }

    fn load_cache(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.claude_dir.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let cache = cx
                .background_executor()
                .spawn(async move { Cache::read(&dir) })
                .await;
            this.update(cx, |view, cx| {
                view.cache = cache;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn load_check(
        &mut self,
        resolve: Option<(String, Resolve)>,
        cx: &mut Context<Self>,
    ) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        let request = match &resolve {
            Some((id, choice)) => {
                if !self.check.start_resolve(id) {
                    return;
                }
                wire::resolve(id, choice.choice())
            }
            None => wire::check(),
        };
        let home = self.home.clone();
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { actions.read_full(&request).map_err(|f| f.message) })
                .await;
            this.update(cx, |view, cx| {
                view.check.show(reply, &home);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn preset_note(&self, profile: &Profile) -> String {
        if profile.custom {
            t::costs::preset_custom_note(&preset_title(&profile.preset))
        } else {
            t::costs::preset_note(&preset_title(&profile.preset))
        }
    }

    fn setup_section(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let open: Click = Rc::new(cx.listener(|_, _, _, cx| cx.emit(OpenSetup)));
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::costs::setup_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(
                card(a)
                    .px(px(14.0))
                    .py(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        t::costs::setup_intro(),
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    )))
                    .child(action(
                        "costs-setup-open",
                        t::costs::setup_open(),
                        ButtonKind::Secondary,
                        self.profile.is_some(),
                        &open,
                        a,
                    )),
            )
    }

    fn tabs(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let options: Vec<(String, String)> = Tab::ALL
            .iter()
            .map(|t| (t.key().to_owned(), t.title().to_owned()))
            .collect();
        let pick: Handler<CostsView, String> = Rc::new(|this, key, cx| {
            if let Some(tab) = Tab::from_key(&key) {
                this.show_tab(tab, cx);
            }
        });
        segmented_full("costs-tab", &options, self.tab.key(), a, cx, pick)
    }

    pub(super) fn preset_section(
        &self,
        profile: &Profile,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let options: Vec<(String, String)> = profile
            .segments()
            .iter()
            .map(|s| (s.key.to_owned(), preset_title(s.key)))
            .collect();
        let picked = profile
            .segments()
            .iter()
            .find(|s| s.selected)
            .map_or(String::new(), |s| s.key.to_owned());
        let pick: Handler<CostsView, String> = Rc::new(|this, key, cx| this.set_preset(&key, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::costs::preset_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(segmented_full(
                "costs-preset",
                &options,
                &picked,
                a,
                cx,
                pick,
            ))
            .child(label(
                &self.preset_note(profile),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
    }
}

pub fn preset_title(key: &str) -> String {
    match key {
        "saver" => t::costs::preset_saver(),
        "balanced" => t::costs::preset_balanced(),
        "quality" => t::costs::preset_quality(),
        _ => t::costs::preset_custom(),
    }
    .to_owned()
}

impl Render for CostsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let mut page = div().flex().flex_col().gap(px(14.0));
        page = page.child(self.setup_section(a, cx));
        if let Some(why) = &self.load_error {
            return page
                .child(label(
                    &t::costs::read_failed(why),
                    TypeRole::Meta,
                    Ink::Needs,
                    a,
                ))
                .part("Costs Page", a, cx);
        }
        let Some(profile) = self.profile.clone() else {
            return page
                .child(label(
                    t::costs::loading(),
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                ))
                .part("Costs Page", a, cx);
        };
        page = page.child(self.tabs(a, cx));
        page = match self.tab {
            Tab::Roles => page
                .child(self.preset_section(&profile, a, cx))
                .child(self.table_section(&profile, a, cx))
                .children(self.picker_section(&profile, a, cx))
                .child(self.habits_section(&profile, a))
                .children(self.imported_section(&profile, a)),
            Tab::Check => page.child(self.check_section(a, cx)),
            Tab::Cache => page.child(self.cache_section(a)),
        };
        page.children(
            self.error
                .as_deref()
                .map(|e| label(e, TypeRole::Meta, Ink::Needs, a)),
        )
        .part("Costs Page", a, cx)
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<gpui::AnyView> {
    use super::renders::{cache, check, options, profile, selection, setup, shown, Shown};
    use super::setup_view::SetupView;
    use crate::acting::Acting;
    use crate::sheets::agents::Page;
    use crate::sheets::settings_view::SettingsView;
    use crate::sheets::view::Acts;
    let which = shown(name)?;
    let acts = Acts {
        actions: Arc::new(Actions::fixed(Acting::Off)),
        sessions: None,
    };
    Some(match which {
        Shown::Setup | Shown::Applied | Shown::SetupOff | Shown::Damaged => {
            cx.new(|_| SetupView::fixed(setup(which))).into()
        }
        _ => {
            let tab = match which {
                Shown::Check | Shown::CheckDamaged => Tab::Check,
                Shown::Cache => Tab::Cache,
                _ => Tab::Roles,
            };
            let picker = selection(which).map(|(r, t)| (r.to_owned(), t));
            let costs = cx.new(|_| {
                CostsView::fixed(
                    profile(which),
                    options(),
                    tab,
                    picker,
                    check(which),
                    cache(),
                )
            });
            cx.new(|_| {
                SettingsView::new(None, acts)
                    .with_page(Page::Costs)
                    .with_costs(costs)
            })
            .into()
        }
    })
}
