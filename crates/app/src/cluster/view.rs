use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    deferred, div, prelude::*, px, AnyView, ClipboardItem, Context, Div, Entity, EventEmitter,
    Render, SharedString, Subscription, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{SIDE_PADDING, SPACE};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon, icon_button, label, menu_panel, menu_rule, scrollable,
    window_appearance, KitPart, KitStyled, MenuItem, TextEditor, TextEditorEvent,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};
use notchling_locale as t;

use super::feed::ClusterFeed;
use super::model::{ClusterModel, NodeRow};
use super::renders::{model, shown};
use super::wire::{self, Request};
use crate::accounts::flows::Alert;
use crate::accounts::view_rows::{alert_overlay, Handler};
use crate::sheets::view::{sheet_header, Dismiss};

const POLL: Duration = Duration::from_millis(500);

pub struct ClusterSheetView {
    feed: Arc<ClusterFeed>,
    menu: Option<String>,
    removing: Option<String>,
    name: Entity<TextEditor>,
    busy: bool,
    note: Option<(String, bool)>,
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
    pub fn live(feed: Arc<ClusterFeed>, cx: &mut Context<Self>) -> ClusterSheetView {
        let view = ClusterSheetView::build(feed, &this_mac(), cx);
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

    pub fn fixed(feed: Arc<ClusterFeed>, name: &str, cx: &mut Context<Self>) -> ClusterSheetView {
        ClusterSheetView::build(feed, name, cx)
    }

    fn build(feed: Arc<ClusterFeed>, name: &str, cx: &mut Context<Self>) -> ClusterSheetView {
        let editor = cx.new(|cx| TextEditor::new(t::cluster::name_label(), 1, cx).single_line());
        editor.update(cx, |editor, cx| editor.set_text(name, cx));
        let events = cx.subscribe(&editor, |this, _, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Submit {
                this.create(cx);
            }
        });
        ClusterSheetView {
            feed,
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

    fn toggle_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        self.menu = if self.menu.as_deref() == Some(id) {
            None
        } else {
            Some(id.to_owned())
        };
        cx.notify();
    }

    fn pick(
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

    fn dot(row: &NodeRow, a: Appearance) -> Div {
        let ink = if row.online && !row.removed {
            Ink::Done
        } else {
            Ink::TextFaint
        };
        div()
            .flex_shrink_0()
            .size(px(SPACE[1]))
            .rounded_full()
            .fill(ink, a)
    }

    fn pill(text: &str, ink: Ink, wash: Ink, a: Appearance) -> Div {
        div()
            .h(px(notchling_kit::metric::PILL_HEIGHT))
            .px(px(SPACE[1]))
            .flex_shrink_0()
            .rounded_full()
            .fill(wash, a)
            .flex()
            .items_center()
            .child(label(text, TypeRole::Label, ink, a))
    }

    fn row_menu(&self, row: &NodeRow, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = row.id.clone();
        let mut items = menu_panel(a)
            .absolute()
            .right(px(SPACE[1]))
            .top(px(SPACE[4] + SPACE[1]));
        let master = id.clone();
        items = items.child(
            MenuItem::new(
                SharedString::from(format!("node-master-{id}")),
                t::cluster::can_be_master(),
            )
            .checked(row.can_be_master)
            .build(a)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pick(&master, ClusterModel::toggle_can_be_master, cx)
            })),
        );
        let acting = id.clone();
        items = items.child(
            MenuItem::new(
                SharedString::from(format!("node-acting-{id}")),
                if row.desired.0 {
                    t::cluster::acting_off()
                } else {
                    t::cluster::acting_on()
                },
            )
            .build(a)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pick(&acting, ClusterModel::toggle_acting, cx)
            })),
        );
        if row.offers_remove() {
            let remove = id.clone();
            items = items.child(menu_rule(a)).child(
                MenuItem::new(
                    SharedString::from(format!("node-remove-{id}")),
                    t::cluster::remove(),
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.menu = None;
                    this.removing = Some(remove.clone());
                    cx.notify();
                })),
            );
        }
        items
    }

    fn node_row(&self, row: &NodeRow, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = row.id.clone();
        let mut title = div()
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .child(Self::dot(row, a))
            .child(label(&row.name, TypeRole::Heading, Ink::Text, a).elide())
            .child(Self::pill(row.role, Ink::AccentText, Ink::AccentWash, a));
        if row.me {
            title = title.child(Self::pill(
                t::cluster::this_mac(),
                Ink::TextMuted,
                Ink::Raised,
                a,
            ));
        }
        let mut head = div()
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .child(div().flex_1().min_w(px(0.0)).child(title));
        head = head.child(label(row.state_word(), TypeRole::Meta, Ink::TextMuted, a));
        if row.offers_menu() {
            let open = self.menu.as_deref() == Some(row.id.as_str());
            head = head.child(
                icon_button(
                    SharedString::from(format!("node-more-{id}")),
                    Icon::More,
                    IconSize::Button.points(),
                    if open {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(&id, cx))),
            );
        }
        let dim = if row.removed {
            Ink::TextFaint
        } else {
            Ink::TextMuted
        };
        let mut about: Vec<String> = Vec::new();
        about.extend((!row.os.is_empty()).then(|| row.os.clone()));
        about.extend(row.app_version.as_deref().map(t::cluster::app_version));
        let mut body = div().flex().flex_col().gap(px(SPACE[0] / 2.0)).child(head);
        if !about.is_empty() {
            body = body.child(label(&about.join(" · "), TypeRole::Meta, dim, a));
        }
        if let Some(load) = row.load_text() {
            body = body.child(label(&load, TypeRole::Meta, dim, a));
        }
        let tools = row.tool_texts();
        if !tools.is_empty() {
            body = body.child(label(&tools.join(" · "), TypeRole::Meta, Ink::TextFaint, a));
        }
        for warning in &row.warnings {
            body = body.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[0]))
                    .child(icon(
                        Icon::Warning,
                        TypeRole::Meta.spec().size,
                        Ink::Needs,
                        a,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        warning,
                        TypeRole::Meta,
                        Ink::Needs,
                        a,
                    ))),
            );
        }
        let menu = (self.menu.as_deref() == Some(row.id.as_str()) && row.offers_menu())
            .then(|| deferred(self.row_menu(row, a, cx)).with_priority(1));
        card(a)
            .relative()
            .p(px(SPACE[2]))
            .child(body)
            .children(menu)
            .part_at("Node Row", Corner::TopTrailing, a, cx)
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

    fn create_form(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
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
                    .h(px(notchling_kit::metric::CONTROL))
                    .px(px(SPACE[1] + 2.0))
                    .flex()
                    .items_center()
                    .rounded(px(notchling_kit::metric::RADIUS_SM))
                    .fill(Ink::Card, a)
                    .border(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .kit_type(TypeRole::Body)
                    .ink(Ink::Text, a)
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
        } else if model.can_create() {
            body = body.child(self.create_form(a, cx));
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
        let click_away = self.menu.is_some().then(|| {
            div()
                .id("cluster-click-away")
                .absolute()
                .size_full()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                }))
        });
        let alert = self.remove_alert(&model, a, cx);
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
                t::cluster::title(),
                div().id("cluster-header-end").child(done),
                a,
            ))
            .child(scrollable(body))
            .children(click_away)
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
    let full = shown(name)?;
    let feed = Arc::new(ClusterFeed::fixed(model(full)));
    let sheet = cx.new(|cx| ClusterSheetView::fixed(feed, "MacBook", cx));
    Some(cx.new(|_| ClusterRender { sheet }).into())
}
