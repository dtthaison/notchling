//! The first-run and About views over GPUI (part U11, macOS), and the pieces they and the handover
//! view share. Every call to the daemon, to `defaults` and to the network runs on the background
//! executor. A render has fixed models and nothing behind it.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyView, Context, Div, Render, SharedString, Window};
use notchling_kit::metric::{RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, claude_mark, hsla, label, window_appearance, KitStyled};
use notchling_kit::{sentence, Appearance, Ink, TypeRole};

use super::about::{current, About};
use super::defaults::{import_into, DefaultsSource};
use super::first_run::{FirstRun, HookBlock, OWNED_BY_SWIFT, OWNERSHIP, TITLE};
use super::hooks::Hooks;
use super::renders::{self, Shown};
use super::updates::{self, FeedFetcher, Outcome};

/// A section's title over its content.
pub fn section(title: &str, a: Appearance) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(label(title, TypeRole::Section, Ink::TextFaint, a))
}

/// A heading and a muted line under it.
pub fn titled_line(title: &str, detail: &str, a: Appearance) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(label(title, TypeRole::Heading, Ink::Text, a))
        .child(label(detail, TypeRole::Meta, Ink::TextMuted, a))
}

/// A list of lines in the mono face, for the changes an install would make.
pub fn mono_lines(lines: &[String], a: Appearance) -> Div {
    div()
        .p(px(10.0))
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, a)
        .flex()
        .flex_col()
        .gap(px(4.0))
        .children(
            lines
                .iter()
                .map(|l| label(l, TypeRole::Mono, Ink::TextMuted, a)),
        )
}

/// What the live first-run view calls.
pub struct FirstRunEnv {
    pub hooks: Arc<Hooks>,
    pub defaults: Arc<dyn DefaultsSource>,
    /// The app's own `settings.json`.
    pub settings: PathBuf,
}

pub struct FirstRunView {
    model: FirstRun,
    env: Option<FirstRunEnv>,
    busy: bool,
}

impl FirstRunView {
    pub fn new(model: FirstRun, env: Option<FirstRunEnv>) -> FirstRunView {
        FirstRunView {
            model,
            env,
            busy: false,
        }
    }

    fn bring_over(&mut self, cx: &mut Context<Self>) {
        let Some(env) = &self.env else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        let (defaults, file) = (env.defaults.clone(), env.settings.clone());
        cx.spawn(async move |this, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { import_into(defaults.as_ref(), &file) })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.model.imported = Some(done);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        let Some(env) = &self.env else {
            return;
        };
        if self.busy || !self.model.hook_block().offers_install() {
            return;
        }
        self.busy = true;
        let hooks = env.hooks.clone();
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { hooks.apply() })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.model.asked = Some(answer);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn hook_card(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let block = self.model.hook_block();
        let mut body = div().flex().flex_col().gap(px(10.0));
        let say = |text: &str, ink: Ink| label(text, TypeRole::Body, ink, a);
        match &block {
            HookBlock::Unknown(why) => body = body.child(say(why, Ink::TextMuted)),
            HookBlock::SwiftOwns { entries } => {
                body = body
                    .child(say(
                        &format!(
                            "The Swift app's hook is in settings.json ({entries} {}). It stays as it is.",
                            if *entries == 1 { "entry" } else { "entries" }
                        ),
                        Ink::Text,
                    ))
                    .child(label(
                        "Two hooks would both answer the same prompt, so installing this app's hook isn't offered while it is there.",
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    ));
            }
            HookBlock::Refused(why) => {
                body = body
                    .child(say(
                        "settings.json can't be edited, so it is left as it is.",
                        Ink::Text,
                    ))
                    .child(label(why, TypeRole::Meta, Ink::Needs, a));
            }
            HookBlock::Installed => {
                body = body.child(say("This app's hook is installed.", Ink::Text));
            }
            HookBlock::Offer {
                changes,
                notes,
                install_on,
                off_message,
            } => {
                body = body.child(say(
                    "No hook sends Claude Code's prompts to this app yet. Installing would:",
                    Ink::Text,
                ));
                body = body.child(mono_lines(changes, a));
                for note in notes {
                    body = body.child(label(note, TypeRole::Meta, Ink::TextMuted, a));
                }
                if !install_on {
                    let why = sentence(off_message.as_deref().unwrap_or("install is off"));
                    body = body.child(label(
                        &format!(
                            "{why}. The button below only shows this plan; nothing is written."
                        ),
                        TypeRole::Meta,
                        Ink::Needs,
                        a,
                    ));
                }
            }
            HookBlock::Asked {
                applied,
                message,
                changes,
                backup,
            } => {
                let head = if *applied {
                    "Installed."
                } else {
                    "Nothing was written. This is what an install would do:"
                };
                body = body.child(say(head, Ink::Text));
                body = body.child(mono_lines(changes, a));
                if let Some(message) = message {
                    body = body.child(label(&sentence(message), TypeRole::Meta, Ink::Needs, a));
                }
                if let Some(backup) = backup {
                    body = body.child(label(
                        &format!("Backup of settings.json: {backup}"),
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    ));
                }
            }
        }
        if block.offers_install() {
            let state = if self.busy {
                PartState::Disabled
            } else {
                PartState::Rest
            };
            body = body.child(
                div().flex().child(
                    button(
                        "install-hook",
                        self.model.install_button(),
                        ButtonKind::Secondary,
                        state,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.install(cx))),
                ),
            );
        }
        section("Claude Code's hook", a).child(card(a).p(px(14.0)).child(body))
    }

    fn settings_card(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let live = self.env.is_some();
        let state = if self.busy || !live {
            PartState::Disabled
        } else {
            PartState::Rest
        };
        section("Your settings", a).child(
            card(a)
                .p(px(14.0))
                .flex()
                .items_center()
                .gap(px(14.0))
                .child(
                    label(&self.model.import_line(), TypeRole::Body, Ink::TextMuted, a)
                        .flex_1()
                        .min_w(px(0.0)),
                )
                .child(
                    button(
                        "bring-over",
                        "Bring Over Settings",
                        ButtonKind::Secondary,
                        state,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.bring_over(cx))),
                ),
        )
    }
}

impl Render for FirstRunView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let owned = OWNED_BY_SWIFT
            .iter()
            .map(|(title, detail)| titled_line(title, detail, a));
        let done = button(
            "first-run-done",
            "Done",
            ButtonKind::Primary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|_, _, window, _| window.remove_window()));
        div()
            .size_full()
            .flex()
            .flex_col()
            .fill(Ink::Window, a)
            .child(
                div()
                    .id("first-run-body")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .p(px(24.0))
                    .flex()
                    .flex_col()
                    .gap(px(20.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.0))
                            .child(claude_mark(40.0, a))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .child(label(TITLE, TypeRole::Title, Ink::Text, a))
                                    .child(label(
                                        &self.model.swift_line(),
                                        TypeRole::Meta,
                                        Ink::TextMuted,
                                        a,
                                    )),
                            ),
                    )
                    .child(
                        section("Who owns what", a).child(
                            card(a)
                                .p(px(14.0))
                                .flex()
                                .flex_col()
                                .gap(px(10.0))
                                .child(label(OWNERSHIP, TypeRole::Body, Ink::Text, a))
                                .children(owned),
                        ),
                    )
                    .child(self.settings_card(a, cx))
                    .child(self.hook_card(a, cx))
                    .child(label(
                        "Open this again from the Notchling menu, under Welcome. Handover is where each area is turned over.",
                        TypeRole::Meta,
                        Ink::TextFaint,
                        a,
                    )),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(SIDE_PADDING + 8.0))
                    .py(px(12.0))
                    .flex()
                    .justify_end()
                    .border_t(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .child(done),
            )
    }
}

/// What the live About view calls.
pub struct AboutEnv {
    pub fetcher: Arc<dyn FeedFetcher>,
    /// The kill switch's value, read once from the environment.
    pub switch: Option<String>,
}

pub struct AboutView {
    about: About,
    env: Option<AboutEnv>,
    outcome: Option<Outcome>,
    checking: bool,
}

impl AboutView {
    pub fn new(about: About, env: Option<AboutEnv>, outcome: Option<Outcome>) -> AboutView {
        AboutView {
            about,
            env,
            outcome,
            checking: false,
        }
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        let Some(env) = &self.env else {
            return;
        };
        if self.checking {
            return;
        }
        self.checking = true;
        let (fetcher, switch) = (env.fetcher.clone(), env.switch.clone());
        let version = self.about.version.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { updates::check(&version, switch.as_deref(), fetcher.as_ref()) })
                .await;
            this.update(cx, |view, cx| {
                view.checking = false;
                view.outcome = Some(outcome);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let fact = |name: &str, value: &str| {
            div()
                .flex()
                .gap(px(12.0))
                .child(label(name, TypeRole::Meta, Ink::TextFaint, a).w(px(70.0)))
                .child(
                    label(value, TypeRole::Label, Ink::Text, a)
                        .flex_1()
                        .min_w(px(0.0)),
                )
        };
        let links = self.about.links.iter().enumerate().map(|(i, link)| {
            let url = link.url;
            button(
                SharedString::from(format!("about-link-{i}")),
                link.title,
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url)))
        });
        let line = match &self.outcome {
            Some(outcome) => outcome.line(),
            None => "Not checked yet.".to_owned(),
        };
        let state = if self.checking || self.env.is_none() {
            PartState::Disabled
        } else {
            PartState::Rest
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(18.0))
            .p(px(28.0))
            .fill(Ink::Window, a)
            .child(claude_mark(64.0, a))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .child(label(self.about.name, TypeRole::Title, Ink::Text, a))
                    .child(label(self.about.tagline, TypeRole::Meta, Ink::TextMuted, a)),
            )
            .child(
                card(a)
                    .w_full()
                    .p(px(14.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(fact(
                        "Version",
                        &format!("{} ({})", self.about.version, self.about.channel),
                    ))
                    .child(fact("Build", &self.about.build))
                    .child(fact("License", self.about.license)),
            )
            .child(
                card(a)
                    .w_full()
                    .p(px(14.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(label("Updates", TypeRole::Section, Ink::TextFaint, a))
                    .child(label(&line, TypeRole::Body, Ink::TextMuted, a))
                    .child(label(
                        "Checking never downloads or installs anything.",
                        TypeRole::Meta,
                        Ink::TextFaint,
                        a,
                    ))
                    .child(
                        div().flex().child(
                            button(
                                "check-updates",
                                if self.checking {
                                    "Checking…"
                                } else {
                                    "Check for Updates"
                                },
                                ButtonKind::Secondary,
                                state,
                                a,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.check(cx))),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(4.0))
                    .children(links),
            )
    }
}

/// The live About view's model: this build, no outcome yet unless the kill switch is off.
pub fn live_about(env: AboutEnv) -> AboutView {
    let outcome = (!updates::releases_on(env.switch.as_deref())).then_some(Outcome::Off);
    AboutView::new(current(), Some(env), outcome)
}

/// `--render first-run|first-run-swift-hook|about`, over made-up data; nothing is called.
pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    Some(match renders::shown(name)? {
        which @ (Shown::FirstRun | Shown::FirstRunSwiftHook) => cx
            .new(|_| FirstRunView::new(renders::first_run(which), None))
            .into(),
        Shown::About => cx
            .new(|_| {
                AboutView::new(
                    renders::about_model(),
                    None,
                    Some(renders::update_outcome()),
                )
            })
            .into(),
        Shown::Handover | Shown::HandoverConfirm => {
            return super::handover_view::build_render(name, cx)
        }
    })
}
