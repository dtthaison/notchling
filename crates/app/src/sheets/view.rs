use notchling_locale as t;
use std::sync::Arc;

use gpui::{
    div, prelude::*, px, Context, Div, EventEmitter, PathPromptOptions, Render, SharedString,
    Stateful, Window,
};
use notchling_kit::metric::{
    CONTROL, RADIUS_CARD, RADIUS_MD, SESSION_LINE_HEADING, SESSION_LINE_META, SESSION_ROW,
    SIDE_PADDING, SPINNER_SMALL, TITLE_BAR, TOOL_ICON,
};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::scrollbar::GUTTER;
use notchling_kit::ui::{
    button, card, hsla, icon, icon as kit_icon, label, scrollable, spinner, tool_icon,
    window_appearance, KitStyled, ToolMark,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::new_session::{folder_title, parent_line, trust_text, NewSession, Plan, Section, Stage};
use crate::accounts::store::Accounts;
use crate::acting::{Acting, Control};
use crate::actions::Actions;
use crate::mac::gate;
use crate::session_source::{Link, SessionSource};
use crate::switches::feed::SwitchFeed;

pub struct Dismiss;

pub struct StartDraft {
    pub provider: String,
    pub folder: String,
}

pub struct StartedJob {
    pub job: String,
}

#[derive(Clone)]
pub struct AccountsHandle {
    pub store: Arc<Accounts>,
    pub feed: Arc<SwitchFeed>,
}

#[derive(Clone)]
pub struct Acts {
    pub actions: Arc<Actions>,
    pub sessions: Option<Arc<SessionSource>>,
}

impl Acts {
    pub fn acting(&self) -> Acting {
        let attached = self
            .sessions
            .as_ref()
            .is_some_and(|s| s.view().link == Link::Daemon);
        self.actions.acting(attached)
    }
}

pub fn sheet_header(icon: Icon, title: &str, end: Stateful<Div>, a: Appearance) -> Div {
    div()
        .h(px(TITLE_BAR + 6.0))
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .flex()
        .items_center()
        .gap(px(10.0))
        .fill(Ink::Card, a)
        .border_b(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .child(kit_icon(
            icon,
            TypeRole::Heading.spec().size,
            Ink::TextMuted,
            a,
        ))
        .child(label(title, TypeRole::Heading, Ink::Text, a))
        .child(div().flex_1())
        .child(end)
}

pub fn choice_row(
    id: impl Into<SharedString>,
    title: &str,
    detail: &str,
    picked: bool,
    a: Appearance,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    div()
        .id(id)
        .w_full()
        .flex_shrink_0()
        .min_h(px(SESSION_ROW))
        .px(px(10.0))
        .rounded(px(RADIUS_MD))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.0))
        .cursor_pointer()
        .when(picked, |r| r.fill(Ink::Selection, a))
        .when(!picked, |r| {
            r.hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
        })
        .child(
            label(title, TypeRole::Label, Ink::Text, a)
                .line_height(px(SESSION_LINE_HEADING))
                .elide(),
        )
        .child(
            label(detail, TypeRole::Meta, Ink::TextFaint, a)
                .line_height(px(SESSION_LINE_META))
                .elide(),
        )
}

fn column_title(text: &str, a: Appearance) -> Div {
    div().px(px(8.0)).pt(px(12.0)).pb(px(4.0)).child(label(
        text,
        TypeRole::Section,
        Ink::TextFaint,
        a,
    ))
}

fn section_title(section: Section, a: Appearance) -> Div {
    let tool = match section {
        Section::Claude => ToolMark::Claude,
        Section::Codex => ToolMark::Codex,
        Section::Antigravity => ToolMark::Antigravity,
    };
    div()
        .px(px(8.0))
        .pt(px(12.0))
        .pb(px(4.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(tool_icon(tool, TOOL_ICON, a))
        .child(label(section.title(), TypeRole::Section, Ink::TextFaint, a))
}

pub struct NewSessionView {
    pub model: NewSession,
    acts: Acts,
    accounts: Option<AccountsHandle>,
}

impl EventEmitter<Dismiss> for NewSessionView {}
impl EventEmitter<StartDraft> for NewSessionView {}
impl EventEmitter<StartedJob> for NewSessionView {}

impl NewSessionView {
    pub fn new(model: NewSession, acts: Acts, accounts: Option<AccountsHandle>) -> NewSessionView {
        NewSessionView {
            model,
            acts,
            accounts,
        }
    }

    fn send(
        &mut self,
        request: crate::acting::Request,
        done: fn(&mut NewSession, Result<(), String>) -> Option<crate::acting::Request>,
        cx: &mut Context<Self>,
    ) {
        let actions = self.acts.actions.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            let job = result
                .as_ref()
                .ok()
                .and_then(|v| v.get("jobId"))
                .and_then(|j| j.as_str())
                .map(str::to_owned);
            let result = result.map(|_| ());
            this.update(cx, |view, cx| {
                if let Some(job) = job {
                    cx.emit(StartedJob { job });
                }
                if let Some(next) = done(&mut view.model, result) {
                    view.send(
                        next,
                        |m, r| {
                            m.started(r);
                            None
                        },
                        cx,
                    );
                }
                if view.model.stage == Stage::Started {
                    cx.emit(Dismiss);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if !self.model.start_control(self.acts.acting()).enabled {
            return;
        }
        let confirm = self.model.confirming();
        match self.model.plan() {
            Some(Plan::Request(request)) => self.send(
                request,
                |m, r| {
                    m.started(r);
                    None
                },
                cx,
            ),
            Some(Plan::Lane { account }) => self.make_lane(account, cx),
            Some(Plan::Draft {
                provider,
                account,
                switch,
                folder,
            }) => {
                if switch {
                    self.switch_then_draft(provider, account, confirm, folder, cx);
                } else {
                    self.model.stage = Stage::Started;
                    cx.emit(StartDraft { provider, folder });
                    cx.emit(Dismiss);
                }
            }
            None => {}
        }
        cx.notify();
    }

    fn make_lane(&mut self, account: String, cx: &mut Context<Self>) {
        let Some(handle) = self.accounts.clone() else {
            self.model
                .lane_made(Err(t::chat::sheet_no_accounts_lane().into()));
            return;
        };
        cx.spawn(async move |this, cx| {
            let made = cx
                .background_executor()
                .spawn(async move {
                    handle
                        .store
                        .create_lane(&handle.feed.switches(), &account)
                        .map(|lane| lane.path)
                })
                .await;
            this.update(cx, |view, cx| {
                if let Some(request) = view.model.lane_made(made) {
                    view.send(
                        request,
                        |m, r| {
                            m.started(r);
                            None
                        },
                        cx,
                    );
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn switch_then_draft(
        &mut self,
        provider: String,
        account: String,
        confirm: bool,
        folder: String,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self.accounts.clone() else {
            self.model
                .switched(Err(t::chat::sheet_no_accounts_switch().into()));
            return;
        };
        let id = provider.clone();
        cx.spawn(async move |this, cx| {
            let switched = cx
                .background_executor()
                .spawn(async move {
                    handle
                        .store
                        .switch_provider(&handle.feed.switches(), &id, &account, confirm)
                })
                .await;
            this.update(cx, |view, cx| {
                if view.model.switched(switched) {
                    cx.emit(StartDraft { provider, folder });
                    cx.emit(Dismiss);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn trust(&mut self, cx: &mut Context<Self>) {
        if !self.acts.acting().control().enabled {
            return;
        }
        if let Some(request) = self.model.trust() {
            self.send(request, |m, r| m.trusted(r), cx);
        }
    }

    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t::chat::sheet_choose().into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                if let Some(path) = paths.first() {
                    let path = path.to_string_lossy().into_owned();
                    this.update(cx, |view, cx| {
                        view.model.add_folder(&path);
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn accounts(&self, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut rows = Vec::new();
        let mut section: Option<Section> = None;
        for (i, account) in self.model.accounts.iter().enumerate() {
            if section != Some(account.section) {
                section = Some(account.section);
                rows.push(section_title(account.section, a).into_any_element());
            }
            let row = choice_row(
                format!("account-{i}"),
                &account.title,
                &account.detail,
                self.model.account == Some(i),
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.pick_account(i);
                cx.notify();
            }));
            rows.push(row.into_any_element());
        }
        div()
            .id("accounts")
            .w(px(230.0))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .pl(px(8.0))
            .pr(px(GUTTER))
            .pt(px(4.0))
            .pb(px(12.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .border_r(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(column_title(t::chat::sheet_account(), a))
            .children(rows)
    }

    fn folders(&self, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let folders = &self.model.folders;
        let rows = folders.iter().enumerate().map(|(i, path)| {
            choice_row(
                format!("folder-{i}"),
                &folder_title(path, folders),
                &parent_line(path, &self.model.home),
                self.model.folder == Some(i),
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.pick_folder(i);
                cx.notify();
            }))
        });
        let choose = div()
            .id("choose-folder")
            .min_h(px(CONTROL))
            .flex()
            .items_center()
            .px(px(10.0))
            .rounded(px(RADIUS_MD))
            .cursor_pointer()
            .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
            .child(label(
                t::chat::sheet_choose_folder(),
                TypeRole::Label,
                Ink::AccentText,
                a,
            ))
            .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx)));
        div()
            .id("folders")
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_y_scroll()
            .pl(px(8.0))
            .pr(px(GUTTER))
            .pt(px(4.0))
            .pb(px(12.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(column_title(t::chat::sheet_folder(), a))
            .children(rows)
            .child(choose)
    }

    fn footer(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let control = self.model.start_control(self.acts.acting());
        let failed = matches!(self.model.stage, Stage::Failed(_) | Stage::Untrusted);
        let line = label(
            &self.model.footer(),
            TypeRole::Meta,
            if failed { Ink::Needs } else { Ink::TextMuted },
            a,
        )
        .flex_1()
        .min_w(px(0.0))
        .line_clamp(2);
        let title = match &self.model.stage {
            Stage::Confirm { button, .. } => button.clone(),
            _ => t::chat::sheet_start().to_owned(),
        };
        let start = button(
            "start-session",
            &title,
            ButtonKind::Primary,
            gate::state(control),
            a,
        );
        let start = gate::tip(start, control, a).when(control.enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.start(cx)))
        });
        div()
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .py(px(12.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .children(
                self.model
                    .busy()
                    .then(|| spinner("start-spin", SPINNER_SMALL, Ink::Working, a)),
            )
            .child(line)
            .child(start)
    }

    fn trust_pop_up(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        let folder = self.model.untrusted_folder()?.to_owned();
        let control = match self.model.stage {
            Stage::Trusting => Control {
                enabled: false,
                tooltip: None,
            },
            _ => self.acts.acting().control(),
        };
        let trust = button(
            "trust-folder",
            t::chat::sheet_trust_start(),
            ButtonKind::Primary,
            gate::state(control),
            a,
        );
        let trust = gate::tip(trust, control, a).when(control.enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.trust(cx)))
        });
        let cancel = button(
            "trust-cancel",
            t::chat::sheet_cancel(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.model.cancel_trust();
            cx.notify();
        }));
        let note = control
            .tooltip
            .map(|why| label(why, TypeRole::Meta, Ink::TextMuted, a));
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
                    card(a)
                        .w(px(400.0))
                        .p(px(20.0))
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(icon(
                                    Icon::Permission,
                                    TypeRole::Heading.spec().size,
                                    Ink::AccentText,
                                    a,
                                ))
                                .child(label(
                                    t::chat::sheet_trust_title(),
                                    TypeRole::Heading,
                                    Ink::Text,
                                    a,
                                )),
                        )
                        .child(label(
                            &trust_text(&folder),
                            TypeRole::Body,
                            Ink::TextMuted,
                            a,
                        ))
                        .children(note)
                        .child(
                            div()
                                .pt(px(6.0))
                                .flex()
                                .justify_end()
                                .gap(px(8.0))
                                .child(cancel)
                                .child(trust),
                        ),
                ),
        )
    }
}

impl Render for NewSessionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let cancel = button(
            "cancel",
            t::chat::sheet_cancel(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss)));
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header(
                Icon::Sessions,
                t::chat::sheet_new_session(),
                cancel,
                a,
            ))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .child(self.accounts(a, cx))
                    .child(scrollable(self.folders(a, cx))),
            )
            .child(self.footer(a, cx))
            .children(self.trust_pop_up(a, cx))
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<gpui::AnyView> {
    use super::renders::{acting, banners, client, hooks, new_session, page, shown, Shown};
    use super::settings_view::{BannersView, SettingsView};
    let which = shown(name)?;
    let acts = Acts {
        actions: Arc::new(Actions::fixed(acting(which))),
        sessions: None,
    };
    Some(match which {
        Shown::Banner => cx.new(|_| BannersView { banners: banners() }).into(),
        Shown::NewSession | Shown::TrustFolder => cx
            .new(|_| NewSessionView::new(new_session(which), acts, None))
            .into(),
        _ => cx
            .new(|_| {
                let view = SettingsView::new(None, acts)
                    .with_hooks(hooks(which))
                    .with_page(page(which));
                match client(which) {
                    Some(part) => view.with_client(part),
                    None => view,
                }
            })
            .into(),
    })
}
