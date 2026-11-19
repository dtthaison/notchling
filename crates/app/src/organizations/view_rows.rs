use gpui::{deferred, div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::{RADIUS_SM, SPACE};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{card, hsla, icon, icon_button, label, menu_panel, KitStyled, MenuItem};
use notchling_kit::{org_color, Appearance, Color, Icon, IconSize, Ink, TypeRole, ORG_COLORS};
use notchling_locale as t;

use super::model::{colour_name, FolderEdit, Open, Row};
use super::view::{plain_button, preview_lines, OrganizationsView};

const DOT: f32 = 26.0;
const SWATCH: f32 = 22.0;

fn on_dot(color: Color) -> Color {
    let luma = 0.299 * f32::from(color.r) + 0.587 * f32::from(color.g) + 0.114 * f32::from(color.b);
    if luma > 150.0 {
        Color::hex(0x1F1B17)
    } else {
        Color::WHITE
    }
}

fn dot(color: &str, letter: &str, a: Appearance) -> Div {
    let fill = org_color(color).color(a);
    div()
        .size(px(DOT))
        .flex_shrink_0()
        .rounded_full()
        .bg(hsla(fill))
        .flex()
        .items_center()
        .justify_center()
        .kit_type(TypeRole::Label)
        .text_color(hsla(on_dot(fill)))
        .child(letter.to_owned())
}

impl OrganizationsView {
    pub(super) fn row_view(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = row.id.clone();
        let more_id = id.clone();
        let more = icon_button(
            SharedString::from(format!("org-more-{id}")),
            Icon::More,
            IconSize::Button.points(),
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.model.toggle_menu(&more_id);
            cx.notify();
        }));
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(dot(&row.color, &row.initial(), a))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(label(&row.name, TypeRole::Heading, Ink::Text, a))
                    .child(label(&row.counts.line(), TypeRole::Meta, Ink::TextMuted, a))
                    .child(label(
                        &self.model.tilde_line(row),
                        TypeRole::Meta,
                        Ink::TextFaint,
                        a,
                    )),
            )
            .children(self.model.editable().then_some(more));
        let mut body = div().flex().flex_col().gap(px(8.0)).child(head);
        if let Some(notice) = self
            .model
            .notice
            .as_ref()
            .filter(|n| n.row.as_deref() == Some(row.id.as_str()))
        {
            body = body.child(label(&notice.text, TypeRole::Label, Ink::Needs, a));
        }
        body = match &self.model.open {
            Open::Rename(open) if *open == row.id => body.child(self.rename_form(a, cx)),
            Open::Colour(open) if *open == row.id => body.child(self.colour_row(row, a, cx)),
            Open::Folders(edit) if edit.id == row.id => {
                body.child(self.folder_editor(row, edit, a, cx))
            }
            Open::ConfirmDelete(open) if *open == row.id => {
                body.child(self.delete_confirm(row, a, cx))
            }
            _ => body,
        };
        let menu = matches!(&self.model.open, Open::Menu(open) if *open == row.id)
            .then(|| deferred(self.row_menu(row, a, cx).occlude()).with_priority(1));
        card(a)
            .relative()
            .p(px(SPACE[2]))
            .child(body)
            .children(menu)
    }

    fn row_menu(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = row.id.clone();
        let item = |key: &str, title: &str| {
            MenuItem::new(format!("org-menu-{key}-{id}"), title.to_owned()).build(a)
        };
        let (rename, colour, folders, delete) = (id.clone(), id.clone(), id.clone(), id.clone());
        menu_panel(a)
            .absolute()
            .right(px(10.0))
            .top(px(54.0))
            .child(
                item("rename", t::organizations::menu_rename()).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.model.begin_rename(&rename);
                        let name = this.model.name_of(&rename);
                        this.name.update(cx, |e, cx| e.set_text(&name, cx));
                        this.name.read(cx).focus(window);
                        cx.notify();
                    },
                )),
            )
            .child(
                item("colour", t::organizations::menu_colour()).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.model.begin_colour(&colour);
                        cx.notify();
                    },
                )),
            )
            .child(
                item("folders", t::organizations::menu_folders()).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.open_folders(&folders, cx);
                        this.folder.read(cx).focus(window);
                    },
                )),
            )
            .child(
                item("delete", t::organizations::menu_delete()).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.model.ask_delete(&delete);
                        cx.notify();
                    },
                )),
            )
    }

    fn rename_form(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(self.field_box(&self.name, a).flex_1())
            .child(
                plain_button(
                    "org-rename-save",
                    t::organizations::save(),
                    ButtonKind::Primary,
                    true,
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.submit_name(cx))),
            )
            .child(self.cancel_button(a, cx))
    }

    fn cancel_button(&self, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        plain_button(
            "org-cancel",
            t::organizations::cancel(),
            ButtonKind::Secondary,
            true,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.model.close();
            cx.notify();
        }))
    }

    fn swatches(
        &self,
        current: &str,
        a: Appearance,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut Self, &str, &mut Context<Self>) + Clone + 'static,
    ) -> Div {
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.0))
            .children(ORG_COLORS.iter().map(|c| {
                let name = c.name;
                let picked = pick.clone();
                let chosen = name == current;
                div()
                    .id(SharedString::from(format!("org-swatch-{name}")))
                    .size(px(SWATCH))
                    .rounded_full()
                    .bg(hsla(c.color(a)))
                    .border(px(2.0))
                    .border_color(if chosen {
                        hsla(Ink::Text.color(a))
                    } else {
                        hsla(Color::CLEAR)
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| picked(this, name, cx)))
            }))
    }

    fn colour_row(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = row.id.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(label(
                &t::organizations::colour_title(&row.name),
                TypeRole::Label,
                Ink::TextMuted,
                a,
            ))
            .child(self.swatches(&row.color, a, cx, move |this, color, cx| {
                this.choose_color(&id, color, cx)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        label(&colour_name(&row.color), TypeRole::Meta, Ink::TextFaint, a).flex_1(),
                    )
                    .child(self.cancel_button(a, cx)),
            )
    }

    fn delete_confirm(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = row.id.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(label(
                &t::organizations::delete_title(&row.name),
                TypeRole::Heading,
                Ink::Text,
                a,
            ))
            .child(label(
                &t::organizations::delete_note(&row.name),
                TypeRole::Label,
                Ink::TextMuted,
                a,
            ))
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        plain_button(
                            "org-delete-confirm",
                            t::organizations::delete_confirm(),
                            ButtonKind::Primary,
                            true,
                            a,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.confirm_delete(&id, cx))),
                    )
                    .child(self.cancel_button(a, cx)),
            )
    }

    fn folder_editor(
        &self,
        row: &Row,
        edit: &FolderEdit,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut list = div().flex().flex_col().gap(px(4.0));
        if edit.drafts.is_empty() {
            list = list.child(label(
                t::organizations::folders_empty(),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ));
        }
        for (i, folder) in edit.drafts.iter().enumerate() {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(RADIUS_SM))
                    .fill(Ink::Raised, a)
                    .child(icon(
                        Icon::Folder,
                        TypeRole::Label.spec().size,
                        Ink::TextMuted,
                        a,
                    ))
                    .child(label(&self.model.tilde(folder), TypeRole::Label, Ink::Text, a).flex_1())
                    .child(
                        icon_button(
                            SharedString::from(format!("org-folder-remove-{i}")),
                            Icon::Close,
                            IconSize::Button.points(),
                            ButtonKind::Ghost,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_folder(i, cx))),
                    ),
            );
        }
        let notes = preview_lines(&edit.preview);
        let can_save = self.model.can_save_folders();
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(label(
                &t::organizations::folders_title(&row.name),
                TypeRole::Heading,
                Ink::Text,
                a,
            ))
            .child(label(
                &t::organizations::folders_note(&row.name),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.field_box(&self.folder, a).flex_1())
                    .child(
                        plain_button(
                            "org-folder-add",
                            t::organizations::folders_add(),
                            ButtonKind::Secondary,
                            true,
                            a,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.add_typed_folder(cx))),
                    ),
            )
            .child(
                div().flex().flex_col().gap(px(3.0)).children(
                    notes
                        .into_iter()
                        .map(|(text, ink)| label(&text, TypeRole::Meta, ink, a)),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        plain_button(
                            "org-folders-save",
                            t::organizations::save(),
                            ButtonKind::Primary,
                            can_save,
                            a,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.save_folders(cx))),
                    )
                    .child(self.cancel_button(a, cx)),
            )
    }

    pub(super) fn new_section(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        if !matches!(self.model.open, Open::New) {
            return div().flex().child(
                plain_button(
                    "org-new",
                    t::organizations::new(),
                    ButtonKind::Secondary,
                    true,
                    a,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.model.begin_new();
                    this.name.update(cx, |e, cx| e.set_text("", cx));
                    this.name.read(cx).focus(window);
                    cx.notify();
                })),
            );
        }
        let current = self
            .model
            .new_color
            .clone()
            .unwrap_or_else(|| "grey".to_owned());
        let notice = self
            .model
            .notice
            .as_ref()
            .filter(|n| n.row.is_none())
            .map(|n| label(&n.text, TypeRole::Label, Ink::Needs, a));
        card(a)
            .p(px(SPACE[2]))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(label(
                t::organizations::new_title(),
                TypeRole::Heading,
                Ink::Text,
                a,
            ))
            .child(self.field_box(&self.name, a))
            .child(self.swatches(&current, a, cx, |this, color, cx| {
                this.model.new_color = Some(color.to_owned());
                cx.notify();
            }))
            .children(notice)
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        plain_button(
                            "org-create",
                            t::organizations::create(),
                            ButtonKind::Primary,
                            true,
                            a,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.submit_name(cx))),
                    )
                    .child(self.cancel_button(a, cx)),
            )
    }
}
