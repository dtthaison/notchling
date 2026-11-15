use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyView, ClipboardItem, Context, Div, Entity, EventEmitter, Render,
    Subscription, Window,
};
use notchling_kit::metric::{CONTROL, RADIUS_SM, SIDE_PADDING, SPACE};
use notchling_kit::part::field_look;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, label, scrollable, window_appearance, KitPart, KitStyled, TextEditor, TextEditorEvent,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::feed::ClusterFeed;
use super::model::ClusterModel;
use super::renders::{model_of, shown, Shown, STANDBY};
use super::secrets::VaultAccess;
use super::secrets_view::{editor_body, editor_footer, editor_title, Editor, SecretsEvent};
use super::view_secrets::{secrets_fixture, Editing};
use super::wire::{self, Request};
use crate::accounts::flows::Alert;
use crate::accounts::view_rows::{alert_overlay, Handler};
use crate::sheets::view::{sheet_header, Dismiss};
use crate::switches::feed::SwitchFeed;
use crate::switches::Switches;
use crate::vault::store::Vault;

const POLL: Duration = Duration::from_millis(500);

pub struct ClusterSheetView {
    pub(super) feed: Arc<ClusterFeed>,
    pub(super) access: VaultAccess,
    pub(super) editing: Option<Editing>,
    pub(super) menu: Option<String>,
    pub(super) removing: Option<String>,
    name: Entity<TextEditor>,
    pub(super) busy: bool,
    pub(super) note: Option<(String, bool)>,
    drawn: u64,
    _name_events: Subscription,
}

impl EventEmitter<Dismiss> for ClusterSheetView {}

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
        ClusterSheetView {
            feed,
            access,
            editing: None,
            menu: None,
            removing: None,
            name: editor,
            busy: false,
            note: None,
            drawn: 0,
            _name_events: events,
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

    fn run(&mut self, request: Request, failed: fn(&str) -> String, cx: &mut Context<Self>) {
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

    fn connection(&self, model: &ClusterModel, a: Appearance, cx: &mut Context<Self>) -> Div {
        let line = model.line();
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
        row
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
            .child(
                div()
                    .min_h(px(CONTROL))
                    .px(px(SPACE[1] + 2.0))
                    .flex()
                    .items_center()
                    .rounded(px(RADIUS_SM))
                    .kit_type(TypeRole::Body)
                    .look(&field_look(focused, a))
                    .child(div().flex_1().min_w(px(0.0)).child(self.name.clone())),
            )
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
        if !self.feed.is_live() && !model.is_loaded() {
            body = body.child(label(
                t::cluster::detached(),
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ));
        } else if !model.is_loaded() {
            body = body.child(label(
                t::cluster::loading(),
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ));
        } else if let Some(error) = model.unbuilt_error() {
            body = body.child(label(error, TypeRole::Body, Ink::Needs, a));
        } else if model.can_create() {
            body = body.child(self.create_form(self.name.read(cx).is_focused(window), a, cx));
        } else {
            body = body.child(self.connection(&model, a, cx));
            for row in model.rows() {
                body = body.child(self.node_row(&row, a, cx));
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
        let done = button(
            "cluster-done",
            t::cluster::done(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss)));
        let alert = self.remove_alert(&model, a, cx);
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
            .child(sheet_header(
                Icon::Server,
                &title,
                div().id("cluster-header-end").child(done),
                a,
            ))
            .child(content)
            .children(footer)
            .children(alert)
            .part("Cluster View", a, cx)
    }
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
    let feed = Arc::new(ClusterFeed::fixed(model_of(which)));
    let access = VaultAccess {
        local: Arc::new(Vault::fixed(Vec::new())),
        switches: Arc::new(SwitchFeed::fixed(Switches::all_on())),
    };
    let sheet = cx.new(|cx| {
        let mut sheet = ClusterSheetView::fixed(feed, access, "MacBook", cx);
        if which == Shown::Menu {
            sheet.menu = Some(STANDBY.to_owned());
        }
        if which == Shown::Secrets {
            sheet.editing = Some(secrets_fixture(&sheet.feed.snapshot()));
        }
        sheet
    });
    Some(cx.new(|_| ClusterRender { sheet }).into())
}
