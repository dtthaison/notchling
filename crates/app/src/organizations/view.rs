use std::sync::Arc;

use gpui::{
    div, prelude::*, px, AnyView, Context, Div, Entity, Render, SharedString, Subscription, Window,
};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, hsla, label, window_appearance, KitPart, KitStyled, TextEditor, TextEditorEvent,
};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;
use serde_json::Value;

use super::model::{Load, Open, OrganizationsModel, Preview};
use super::renders::{model as fixture, shown, texts};
use super::wire;
use crate::acting::{Acting, Request};
use crate::actions::{Actions, SendFailure};
use crate::sheets::agents::Page;
use crate::sheets::settings_view::SettingsView;
use crate::sheets::view::Acts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Done {
    Created,
    Edited,
    Deleted,
}

pub struct OrganizationsView {
    pub(super) model: OrganizationsModel,
    actions: Option<Arc<Actions>>,
    pub(super) name: Entity<TextEditor>,
    pub(super) folder: Entity<TextEditor>,
    _events: Vec<Subscription>,
}

impl OrganizationsView {
    pub fn live(actions: Arc<Actions>, cx: &mut Context<Self>) -> OrganizationsView {
        let model = OrganizationsModel {
            home: std::env::var("HOME").unwrap_or_default(),
            ..OrganizationsModel::default()
        };
        let mut view = OrganizationsView::build(model, Some(actions), cx);
        view.refresh(cx);
        view
    }

    pub fn fixed(
        model: OrganizationsModel,
        name: &str,
        folder: &str,
        cx: &mut Context<Self>,
    ) -> OrganizationsView {
        let view = OrganizationsView::build(model, None, cx);
        view.name.update(cx, |editor, cx| editor.set_text(name, cx));
        view.folder
            .update(cx, |editor, cx| editor.set_text(folder, cx));
        view
    }

    fn build(
        model: OrganizationsModel,
        actions: Option<Arc<Actions>>,
        cx: &mut Context<Self>,
    ) -> OrganizationsView {
        let name =
            cx.new(|cx| TextEditor::new(t::organizations::name_placeholder(), 1, cx).single_line());
        let folder = cx.new(|cx| {
            TextEditor::new(t::organizations::folders_placeholder(), 1, cx).single_line()
        });
        let on_name = cx.subscribe(&name, |this, _, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Submit {
                this.submit_name(cx);
            }
        });
        let on_folder = cx.subscribe(&folder, |this, _, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Submit {
                this.add_typed_folder(cx);
            }
        });
        OrganizationsView {
            model,
            actions,
            name,
            folder,
            _events: vec![on_name, on_folder],
        }
    }

    fn detached(&self) -> bool {
        matches!(&self.model.load, Load::Failed(why) if Some(why.as_str()) == Acting::Detached.message())
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        if !self.model.start() {
            return;
        }
        let request = wire::list();
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { actions.read(&request) })
                .await;
            this.update(cx, |view, cx| {
                if view.model.finish(reply) {
                    view.refresh(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn call(&mut self, request: Request, row: Option<String>, done: Done, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { actions.read_full(&request) })
                .await;
            this.update(cx, |view, cx| view.landed(row, done, reply, cx))
                .ok();
        })
        .detach();
    }

    fn landed(
        &mut self,
        row: Option<String>,
        done: Done,
        reply: Result<Value, SendFailure>,
        cx: &mut Context<Self>,
    ) {
        match reply {
            Ok(_) => {
                self.model.close();
                self.name.update(cx, |e, cx| e.set_text("", cx));
                self.folder.update(cx, |e, cx| e.set_text("", cx));
                if done == Done::Created {
                    self.model.new_color = None;
                }
            }
            Err(failure) => {
                self.model
                    .fail(row.as_deref(), &failure.message, failure.data.as_ref());
            }
        }
        self.refresh(cx);
        cx.notify();
    }

    pub(super) fn submit_name(&mut self, cx: &mut Context<Self>) {
        let typed = self.name.read(cx).text().to_owned();
        match self.model.open.clone() {
            Open::New => {
                let color = self.model.new_color.clone();
                self.call(
                    wire::create(&typed, color.as_deref()),
                    None,
                    Done::Created,
                    cx,
                );
            }
            Open::Rename(id) => {
                self.call(wire::rename(&id, &typed), Some(id), Done::Edited, cx);
            }
            _ => {}
        }
    }

    pub(super) fn choose_color(&mut self, id: &str, color: &str, cx: &mut Context<Self>) {
        self.call(
            wire::set_color(id, color),
            Some(id.to_owned()),
            Done::Edited,
            cx,
        );
    }

    pub(super) fn confirm_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        self.call(wire::delete(id), Some(id.to_owned()), Done::Deleted, cx);
    }

    pub(super) fn save_folders(&mut self, cx: &mut Context<Self>) {
        if !self.model.can_save_folders() {
            return;
        }
        if let Open::Folders(edit) = self.model.open.clone() {
            let id = edit.id.clone();
            self.call(
                wire::set_folders(&id, &edit.drafts),
                Some(id),
                Done::Edited,
                cx,
            );
        }
    }

    pub(super) fn open_folders(&mut self, id: &str, cx: &mut Context<Self>) {
        self.model.begin_folders(id);
        self.folder.update(cx, |e, cx| e.set_text("", cx));
        cx.notify();
    }

    pub(super) fn add_typed_folder(&mut self, cx: &mut Context<Self>) {
        let typed = self.folder.read(cx).text().to_owned();
        if self.model.add_folder(&typed) {
            self.folder.update(cx, |e, cx| e.set_text("", cx));
            self.check_folders(cx);
        }
        cx.notify();
    }

    pub(super) fn remove_folder(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.model.remove_folder(index) {
            self.check_folders(cx);
        }
        cx.notify();
    }

    fn check_folders(&mut self, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        let Open::Folders(edit) = &self.model.open else {
            return;
        };
        let (id, drafts) = (edit.id.clone(), edit.drafts.clone());
        let request = wire::preview_folders(&id, &drafts);
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { actions.read_full(&request) })
                .await;
            this.update(cx, |view, cx| {
                let reply = reply.map_err(|f| (f.message, f.data));
                view.model.finish_preview(&id, &drafts, reply);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn field_box(&self, editor: &Entity<TextEditor>, a: Appearance) -> Div {
        div()
            .h(px(30.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .rounded(px(RADIUS_SM))
            .fill(Ink::Card, a)
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .kit_type(TypeRole::Body)
                    .ink(Ink::Text, a)
                    .child(editor.clone()),
            )
    }
}

impl Render for OrganizationsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let mut page = div().flex().flex_col().gap(px(10.0));
        page = page
            .child(label(
                t::organizations::section(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(label(
                t::organizations::intro(),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ));
        if let Some(why) = &self.model.unreadable {
            page = page.child(label(
                &t::organizations::unreadable(why),
                TypeRole::Label,
                Ink::Needs,
                a,
            ));
        }
        if let Some(notice) = self.model.notice.as_ref().filter(|n| n.page) {
            page = page.child(label(&notice.text, TypeRole::Label, Ink::Needs, a));
        }
        match &self.model.load {
            Load::Failed(why) => {
                let text = if self.detached() {
                    t::organizations::detached().to_owned()
                } else {
                    t::organizations::read_failed(why)
                };
                page = page.child(label(&text, TypeRole::Label, Ink::Needs, a));
            }
            Load::Loading => {
                page = page.child(label(
                    t::organizations::loading(),
                    TypeRole::Label,
                    Ink::TextMuted,
                    a,
                ));
            }
            Load::Idle => {}
        }
        let rows: Vec<_> = self.model.rows.clone();
        for row in &rows {
            page = page.child(self.row_view(row, a, cx));
        }
        if self.model.editable() && self.model.load != Load::Loading {
            page = page.child(self.new_section(a, cx));
        }
        if self.model.only_personal() && !self.model.rows.is_empty() && self.model.editable() {
            page = page.child(label(
                t::organizations::only_personal(),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ));
        }
        page.part("Organizations", a, cx)
    }
}

pub(super) fn preview_lines(preview: &Preview) -> Vec<(String, Ink)> {
    match preview {
        Preview::Checking => vec![(
            t::organizations::folders_checking().to_owned(),
            Ink::TextFaint,
        )],
        Preview::Refused(text) => vec![(text.clone(), Ink::Needs)],
        Preview::Ready(notes) => {
            let mut lines: Vec<(String, Ink)> = notes
                .overlaps
                .iter()
                .map(|o| (o.clone(), Ink::TextMuted))
                .collect();
            lines.push((notes.head.clone(), Ink::TextMuted));
            lines.extend(notes.moved.iter().map(|m| (m.clone(), Ink::TextFaint)));
            lines.extend(notes.more.iter().map(|m| (m.clone(), Ink::TextFaint)));
            lines
        }
    }
}

pub(super) fn plain_button(
    id: impl Into<SharedString>,
    text: &str,
    kind: ButtonKind,
    enabled: bool,
    a: Appearance,
) -> gpui::Stateful<Div> {
    let state = if enabled {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    button(id.into(), text, kind, state, a)
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let which = shown(name)?;
    let acts = Acts {
        actions: Arc::new(Actions::fixed(Acting::Off)),
        sessions: None,
    };
    let (name_text, folder_text) = texts(which);
    let organizations =
        cx.new(|cx| OrganizationsView::fixed(fixture(which), name_text, folder_text, cx));
    Some(
        cx.new(|_| {
            SettingsView::new(None, acts)
                .with_page(Page::Organizations)
                .with_organizations(organizations)
        })
        .into(),
    )
}
