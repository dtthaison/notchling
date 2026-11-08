//! The sheets and pop-ups over GPUI (parts U7 and U8, macOS): the New Session sheet with its Trust
//! Folder pop-up, and the banners. Start and Trust Folder and Start send through the daemon
//! (`session.new`, `folder.trust`) and are disabled, with the reason on hover, while acting isn't on.

use std::sync::Arc;

use gpui::{
    div, prelude::*, px, Context, Div, EventEmitter, PathPromptOptions, Render, SharedString,
    Stateful, Window,
};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, SIDE_PADDING, TITLE_BAR};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::new_session::{folder_title, parent_line, trust_text, NewSession, Stage, TRUST_TITLE};
use crate::acting::{Acting, Control};
use crate::actions::Actions;
use crate::mac::gate;
use crate::session_source::{Link, SessionSource};

/// A sheet asks to close.
pub struct Dismiss;

/// Where a sheet's acting comes from: the app's actions and whether the daemon serves the rows.
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

/// A sheet's header: icon, title, and a button on the right.
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
        .child(label(icon.glyph(), TypeRole::Heading, Ink::TextMuted, a))
        .child(label(title, TypeRole::Heading, Ink::Text, a))
        .child(div().flex_1())
        .child(end)
}

/// A pickable row in a column: a title over a faint detail, filled while picked.
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
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(RADIUS_MD))
        .flex()
        .flex_col()
        .gap(px(1.0))
        .cursor_pointer()
        .when(picked, |r| r.fill(Ink::Selection, a))
        .when(!picked, |r| {
            r.hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
        })
        .child(label(title, TypeRole::Label, Ink::Text, a).truncate())
        .child(label(detail, TypeRole::Meta, Ink::TextFaint, a).truncate())
}

fn column_title(text: &str, a: Appearance) -> Div {
    div().px(px(10.0)).pt(px(4.0)).pb(px(6.0)).child(label(
        text,
        TypeRole::Section,
        Ink::TextFaint,
        a,
    ))
}

/// The New Session sheet.
pub struct NewSessionView {
    pub model: NewSession,
    acts: Acts,
}

impl EventEmitter<Dismiss> for NewSessionView {}

impl NewSessionView {
    pub fn new(model: NewSession, acts: Acts) -> NewSessionView {
        NewSessionView { model, acts }
    }

    /// Sends `request`, then hands the answer to `done`; a follow-up request it returns is sent too.
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
                .spawn(async move { actions.send(&request).map(|_| ()) })
                .await;
            this.update(cx, |view, cx| {
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
        if let Some(request) = self.model.start() {
            self.send(
                request,
                |m, r| {
                    m.started(r);
                    None
                },
                cx,
            );
        }
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
            prompt: Some("Choose".into()),
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

    fn accounts(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let rows = self.model.accounts.iter().enumerate().map(|(i, account)| {
            choice_row(
                format!("account-{i}"),
                &account.title,
                &account.detail,
                self.model.account == Some(i),
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.pick_account(i);
                cx.notify();
            }))
        });
        div()
            .w(px(230.0))
            .flex_shrink_0()
            .h_full()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .border_r(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(column_title("Account", a))
            .child(column_title("Claude Code", a))
            .children(rows)
    }

    fn folders(&self, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let folders = &self.model.folders;
        let rows = folders.iter().enumerate().map(|(i, path)| {
            choice_row(
                format!("folder-{i}"),
                &format!(
                    "{}  {}",
                    Icon::Sessions.glyph(),
                    folder_title(path, folders)
                ),
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
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(RADIUS_MD))
            .cursor_pointer()
            .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
            .child(label("Choose Folder…", TypeRole::Label, Ink::AccentText, a))
            .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx)));
        div()
            .id("folders")
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_y_scroll()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(column_title("Folder", a))
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
        let start = button(
            "start-session",
            "Start Session",
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
            .child(line)
            .child(start)
    }

    /// The Trust Folder pop-up, over the dimmed sheet.
    fn trust_pop_up(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        let folder = self
            .model
            .untrusted_folder()
            .map(str::to_owned)
            .or_else(|| (self.model.stage == Stage::Trusting).then(String::new))?;
        let control = match self.model.stage {
            Stage::Trusting => Control {
                enabled: false,
                tooltip: None,
            },
            _ => self.acts.acting().control(),
        };
        let trust = button(
            "trust-folder",
            "Trust Folder and Start",
            ButtonKind::Primary,
            gate::state(control),
            a,
        );
        let trust = gate::tip(trust, control, a).when(control.enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.trust(cx)))
        });
        let cancel = button(
            "trust-cancel",
            "Cancel",
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
                                .child(label(
                                    Icon::Permission.glyph(),
                                    TypeRole::Heading,
                                    Ink::AccentText,
                                    a,
                                ))
                                .child(label(TRUST_TITLE, TypeRole::Heading, Ink::Text, a)),
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
            "Cancel",
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
            .child(sheet_header(Icon::Sessions, "New Session", cancel, a))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .child(self.accounts(a, cx))
                    .child(self.folders(a, cx)),
            )
            .child(self.footer(a, cx))
            .children(self.trust_pop_up(a, cx))
    }
}

/// `--render banner|new-session|trust-folder|settings`, over the fixture with acting as the
/// render says; nothing is ever sent.
pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<gpui::AnyView> {
    use super::renders::{acting, banners, new_session, shown, Shown};
    use super::settings_view::{BannersView, SettingsView};
    let which = shown(name)?;
    let acts = Acts {
        actions: Arc::new(Actions::fixed(acting(which))),
        sessions: None,
    };
    Some(match which {
        Shown::Banner => cx.new(|_| BannersView { banners: banners() }).into(),
        Shown::NewSession | Shown::TrustFolder => cx
            .new(|_| NewSessionView::new(new_session(which), acts))
            .into(),
        Shown::Settings => cx.new(|_| SettingsView::new(None, acts)).into(),
    })
}
