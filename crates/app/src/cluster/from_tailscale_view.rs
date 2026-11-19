use std::collections::BTreeMap;

use gpui::{div, prelude::*, px, Context, Div, Entity, SharedString, Subscription, Window};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon_button, label, KitPart, TextEditor, TextEditorEvent,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};
use notchling_locale as t;

use super::add_parts::AddParts;
use super::from_tailscale::{import_machines, FromTailscale, Read};
use super::view::{field_box, ClusterSheetView};

const SHEET_WIDTH: f32 = 600.0;
const SHEET_MARGIN: f32 = 60.0;

#[derive(Default)]
pub(super) struct TailscaleUi {
    pub model: Option<FromTailscale>,
    pub parts: Option<AddParts>,
    pub filter: Option<Entity<TextEditor>>,
    pub users: BTreeMap<String, Entity<TextEditor>>,
    pub subscriptions: Vec<Subscription>,
}

impl ClusterSheetView {
    pub(super) fn open_tailscale(&mut self, cx: &mut Context<Self>) {
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let model = FromTailscale::new(&parts.this.user, parts.registry.all());
        self.start_tailscale(model, Some(parts.clone()), cx);
        let tailscale = parts.engine.router().tailscale().cloned();
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let (read, organizations) = cx
                .background_executor()
                .spawn(async move {
                    let read = tailscale.map(|t| (t.status(), t.networks().unwrap_or_default()));
                    let organizations = {
                        let request = crate::organizations::wire::list();
                        feed.send(&super::wire::Request {
                            op: request.op,
                            args: request.args,
                        })
                    };
                    (read, organizations)
                })
                .await;
            this.update(cx, |view, cx| {
                if let Some(model) = view.tailscale.model.as_mut() {
                    if let Some((status, networks)) = read {
                        model.loaded(status, networks);
                    }
                    model.set_organizations(organizations);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn start_tailscale(
        &mut self,
        model: FromTailscale,
        parts: Option<AddParts>,
        cx: &mut Context<Self>,
    ) {
        let filter = cx.new(|cx| {
            TextEditor::new(t::cluster::tailscale_filter_placeholder(), 1, cx).single_line()
        });
        let sub = cx.subscribe(&filter, |this, editor, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Changed {
                let text = editor.read(cx).text().to_owned();
                if let Some(model) = this.tailscale.model.as_mut() {
                    model.filter.text = text;
                }
                cx.notify();
            }
        });
        self.tailscale = TailscaleUi {
            model: Some(model),
            parts,
            filter: Some(filter),
            users: BTreeMap::new(),
            subscriptions: vec![sub],
        };
        cx.notify();
    }

    fn tailscale_toggle(&mut self, host: &str, cx: &mut Context<Self>) {
        let Some(model) = self.tailscale.model.as_mut() else {
            return;
        };
        model.toggle(host);
        let user = model.chosen.get(host).cloned();
        match user {
            Some(user) => {
                let editor = cx.new(|cx| TextEditor::new("", 1, cx).single_line());
                editor.update(cx, |editor, cx| editor.set_text(&user, cx));
                let key = host.to_owned();
                let sub =
                    cx.subscribe(&editor, move |this, editor, event: &TextEditorEvent, cx| {
                        if *event == TextEditorEvent::Changed {
                            let text = editor.read(cx).text().to_owned();
                            if let Some(model) = this.tailscale.model.as_mut() {
                                model.set_user(&key, &text);
                            }
                            cx.notify();
                        }
                    });
                self.tailscale.subscriptions.push(sub);
                self.tailscale.users.insert(host.to_owned(), editor);
            }
            None => {
                self.tailscale.users.remove(host);
            }
        }
        cx.notify();
    }

    fn tailscale_select_all(&mut self, cx: &mut Context<Self>) {
        let Some(model) = self.tailscale.model.as_mut() else {
            return;
        };
        model.select_all();
        let chosen: Vec<(String, String)> = model
            .chosen
            .iter()
            .map(|(h, u)| (h.clone(), u.clone()))
            .collect();
        self.tailscale
            .users
            .retain(|h, _| chosen.iter().any(|(c, _)| c == h));
        for (host, user) in chosen {
            if self.tailscale.users.contains_key(&host) {
                continue;
            }
            let editor = cx.new(|cx| TextEditor::new("", 1, cx).single_line());
            editor.update(cx, |editor, cx| editor.set_text(&user, cx));
            let key = host.clone();
            let sub = cx.subscribe(&editor, move |this, editor, event: &TextEditorEvent, cx| {
                if *event == TextEditorEvent::Changed {
                    let text = editor.read(cx).text().to_owned();
                    if let Some(model) = this.tailscale.model.as_mut() {
                        model.set_user(&key, &text);
                    }
                    cx.notify();
                }
            });
            self.tailscale.subscriptions.push(sub);
            self.tailscale.users.insert(host, editor);
        }
        cx.notify();
    }

    fn tailscale_cycle_os(&mut self, cx: &mut Context<Self>) {
        let Some(model) = self.tailscale.model.as_mut() else {
            return;
        };
        let mut options: Vec<Option<String>> = vec![None];
        options.extend(model.systems().into_iter().map(Some));
        model.filter.os = super::addresses_form::cycle(&options, &model.filter.os);
        cx.notify();
    }

    fn tailscale_import(&mut self, cx: &mut Context<Self>) {
        let (Some(model), Some(parts)) =
            (self.tailscale.model.as_mut(), self.tailscale.parts.clone())
        else {
            return;
        };
        if !model.can_import() {
            return;
        }
        let organization = model.organization_choice();
        match import_machines(
            &parts,
            &model.status,
            &model.picked(),
            organization.as_deref(),
        ) {
            Ok(count) => {
                model.imported(count);
                model.existing = parts.registry.all();
                self.tailscale.users.clear();
                self.add.forget_list();
            }
            Err(why) => model.error = Some(why),
        }
        cx.notify();
    }

    fn tailscale_close(&mut self, cx: &mut Context<Self>) {
        self.tailscale = TailscaleUi::default();
        cx.notify();
    }

    pub(super) fn tailscale_overlay(
        &self,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let model = self.tailscale.model.as_ref()?;
        let mut inner = div().flex().flex_col().gap(px(SPACE[1]));
        match &model.read {
            Read::Reading => {
                inner = inner.child(label(
                    t::cluster::tailscale_reading(),
                    TypeRole::Body,
                    Ink::TextMuted,
                    a,
                ));
            }
            Read::Failed(why) => {
                inner = inner.child(label(
                    &t::cluster::tailscale_failed(why),
                    TypeRole::Body,
                    Ink::Needs,
                    a,
                ));
            }
            Read::Ready => inner = self.tailscale_list(inner, model, window, a, cx),
        }
        if let Some(error) = &model.error {
            inner = inner.child(label(error, TypeRole::Meta, Ink::Needs, a));
        }
        if let Some(count) = model.done {
            let text = if count == 0 {
                t::cluster::tailscale_imported_none().to_owned()
            } else {
                t::cluster::tailscale_imported(count as i64)
            };
            inner = inner.child(label(&text, TypeRole::Meta, Ink::TextMuted, a));
        }
        let body = div()
            .id("tailscale-body")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .child(inner);
        let count = model.chosen.len();
        let import_title = if count == 0 {
            t::cluster::tailscale_import_none().to_owned()
        } else {
            t::cluster::tailscale_import(count as i64)
        };
        let import_state = if model.can_import() {
            PartState::Rest
        } else {
            PartState::Disabled
        };
        let buttons = div()
            .pt(px(SPACE[1]))
            .flex()
            .justify_end()
            .gap(px(SPACE[1]))
            .child(
                button(
                    "tailscale-close",
                    t::cluster::done(),
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.tailscale_close(cx))),
            )
            .child(
                button(
                    "tailscale-import",
                    &import_title,
                    ButtonKind::Primary,
                    import_state,
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.tailscale_import(cx))),
            );
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
                        .w(px(SHEET_WIDTH))
                        .max_h(px(super::HEIGHT - SHEET_MARGIN))
                        .p(px(SPACE[3]))
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[1]))
                        .child(label(
                            t::cluster::tailscale_title(),
                            TypeRole::Heading,
                            Ink::Text,
                            a,
                        ))
                        .child(body)
                        .child(buttons)
                        .part_at("From Tailscale Sheet", Corner::TopTrailing, a, cx),
                ),
        )
    }

    fn tailscale_list(
        &self,
        mut inner: Div,
        model: &FromTailscale,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        inner = inner.child(label(
            &t::cluster::tailscale_network(&model.network_name()),
            TypeRole::Label,
            Ink::Text,
            a,
        ));
        if let Some(note) = model.other_networks_note() {
            inner = inner.child(label(&note, TypeRole::Meta, Ink::TextMuted, a));
        }
        let hidden = model.hidden_count();
        if hidden > 0 {
            inner = inner.child(label(
                &t::cluster::tailscale_hidden(hidden as i64),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ));
        }
        if model.offers_organizations() {
            inner = inner.child(
                div().flex().child(
                    button(
                        "tailscale-organization",
                        &t::cluster::tailscale_organization(&model.organization_name()),
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(model) = this.tailscale.model.as_mut() {
                            model.next_organization();
                        }
                        cx.notify();
                    })),
                ),
            );
        }
        if let Some(filter) = &self.tailscale.filter {
            let focused = filter.read(cx).is_focused(window);
            inner = inner.child(field_box(filter, focused, a));
        }
        let os_title = model
            .filter
            .os
            .clone()
            .unwrap_or_else(|| t::cluster::tailscale_os_any().to_owned());
        let online_title = if model.filter.online_only {
            t::cluster::tailscale_online_only()
        } else {
            t::cluster::tailscale_online_all()
        };
        let select_title = if model.all_chosen() {
            t::cluster::tailscale_select_none()
        } else {
            t::cluster::tailscale_select_all()
        };
        inner = inner.child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[0]))
                .child(
                    button(
                        "tailscale-os",
                        &os_title,
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.tailscale_cycle_os(cx))),
                )
                .child(
                    button(
                        "tailscale-online",
                        online_title,
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(model) = this.tailscale.model.as_mut() {
                            model.filter.online_only = !model.filter.online_only;
                        }
                        cx.notify();
                    })),
                )
                .child(div().flex_1())
                .child(
                    button(
                        "tailscale-select-all",
                        select_title,
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.tailscale_select_all(cx))),
                ),
        );
        let rows = model.rows();
        if rows.is_empty() {
            let text = if model.offered().is_empty() {
                t::cluster::tailscale_empty()
            } else {
                t::cluster::tailscale_no_match()
            };
            inner = inner.child(label(text, TypeRole::Body, Ink::TextMuted, a));
        }
        for (i, row) in rows.iter().enumerate() {
            let host = row.host.clone();
            let state = if row.online {
                t::cluster::tailscale_online()
            } else {
                t::cluster::tailscale_offline()
            };
            let mut line = div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(
                    icon_button(
                        SharedString::from(format!("tailscale-pick-{i}")),
                        if row.chosen {
                            Icon::Check
                        } else {
                            Icon::Circle
                        },
                        IconSize::Button.points(),
                        if row.chosen {
                            ButtonKind::Primary
                        } else {
                            ButtonKind::Ghost
                        },
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.tailscale_toggle(&host, cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .child(label(
                            &t::cluster::tailscale_row_title(&row.title, state),
                            TypeRole::Body,
                            if row.online {
                                Ink::Text
                            } else {
                                Ink::TextMuted
                            },
                            a,
                        ))
                        .child(label(&row.detail, TypeRole::Meta, Ink::TextMuted, a))
                        .children(
                            (!row.owner.is_empty())
                                .then(|| label(&row.owner, TypeRole::Meta, Ink::TextFaint, a)),
                        ),
                );
            if let Some(editor) = self.tailscale.users.get(&row.host) {
                let focused = editor.read(cx).is_focused(window);
                line = line.child(
                    div()
                        .w(px(150.0))
                        .flex()
                        .flex_col()
                        .child(label(
                            t::cluster::tailscale_user_label(),
                            TypeRole::Meta,
                            Ink::TextFaint,
                            a,
                        ))
                        .child(field_box(editor, focused, a)),
                );
            }
            inner = inner.child(line);
        }
        inner.child(label(
            t::cluster::tailscale_import_note(),
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ))
    }
}
