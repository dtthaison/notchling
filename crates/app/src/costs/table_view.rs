use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_SM};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, icon_button, label, KitStyled, MenuItem};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::picks::{choose_model, effort_allowed, model_allowed, pick_text, NO_MODEL};
use super::profile::{note_text, role_title, tool_title, Cell, Floor, Profile, Row};
use super::setup::short_path;
use super::view::CostsView;
use crate::mac::gate;

const ROLE_WIDTH: f32 = 92.0;

fn dot(ink: Ink, a: Appearance) -> Div {
    div()
        .flex_shrink_0()
        .size(px(6.0))
        .rounded_full()
        .fill(ink, a)
}

fn cell_texts(profile: &Profile, row: &Row, tool: usize) -> (String, String, bool) {
    if !profile.tools[tool] {
        return (t::costs::tool_off().to_owned(), String::new(), false);
    }
    match &row.cells[tool] {
        None => (t::costs::cell_not_set().to_owned(), String::new(), false),
        Some(cell) => (
            cell.model
                .clone()
                .unwrap_or_else(|| t::costs::cell_no_model().to_owned()),
            cell.effort.clone(),
            true,
        ),
    }
}

fn cell_tip(cell: &Cell) -> Option<String> {
    let mut lines: Vec<String> = cell.notes.iter().filter_map(note_text).collect();
    if cell.own {
        lines.insert(0, t::costs::cell_own().to_owned());
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

impl CostsView {
    fn cell_view(
        &self,
        profile: &Profile,
        row: &Row,
        tool: usize,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (title, effort, set) = cell_texts(profile, row, tool);
        let selected = self
            .picker
            .as_ref()
            .is_some_and(|(r, t)| *r == row.role && *t == tool);
        let id = SharedString::from(format!("costs-cell-{}-{tool}", row.role));
        let cell = row.cells[tool].as_ref();
        let ink = if set { Ink::Text } else { Ink::TextFaint };
        let mut top = div().flex().items_center().gap(px(5.0)).child(
            label(&title, TypeRole::Label, ink, a)
                .elide()
                .min_w(px(0.0)),
        );
        if let Some(cell) = cell {
            if cell.own {
                top = top.child(dot(Ink::Accent, a));
            }
            if !cell.notes.is_empty() {
                top = top.child(dot(Ink::Needs, a));
            }
        }
        let mut shown = div()
            .id(id)
            .flex_1()
            .min_w(px(0.0))
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(RADIUS_SM))
            .flex()
            .flex_col()
            .when(selected, |c| c.fill(Ink::AccentWash, a))
            .child(top)
            .child(label(&effort, TypeRole::Meta, Ink::TextMuted, a).h(px(14.0)));
        if profile.tools[tool] {
            let role = row.role.clone();
            shown = shown
                .cursor_pointer()
                .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
                .on_click(cx.listener(move |this, _, _, cx| this.open_picker(&role, tool, cx)));
        }
        match cell.and_then(cell_tip) {
            Some(tip) => gate::tip_text(shown, SharedString::from(tip), a),
            None => shown,
        }
    }

    fn role_cell(row: &Row, a: Appearance) -> Div {
        let mut col = div()
            .w(px(ROLE_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(label(&role_title(&row.role), TypeRole::Body, Ink::Text, a));
        if let Some(floor) = &row.floor {
            col = col.child(label(
                &t::costs::floor_line(&floor.text()),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ));
        }
        col
    }

    pub(super) fn table_section(
        &self,
        profile: &Profile,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let header = div()
            .px(px(14.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .rounded_t(px(RADIUS_CARD))
            .fill(Ink::Raised, a)
            .child(div().w(px(ROLE_WIDTH)).flex_shrink_0())
            .children((0..3).map(|tool| {
                label(tool_title(tool), TypeRole::Meta, Ink::TextMuted, a)
                    .flex_1()
                    .px(px(8.0))
            }));
        let mut rows = card(a).flex().flex_col().overflow_hidden().child(header);
        for row in &profile.rows {
            let cells: Vec<Stateful<Div>> = (0..3)
                .map(|tool| self.cell_view(profile, row, tool, a, cx))
                .collect();
            rows = rows.child(
                div()
                    .px(px(14.0))
                    .py(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .border_t(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .child(Self::role_cell(row, a))
                    .children(cells),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::costs::roles_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(rows)
            .child(label(
                t::costs::roles_footer(),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
    }

    fn model_rows(
        &self,
        role: &str,
        tool: usize,
        cell: Option<&Cell>,
        floor: Option<&Floor>,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let allow_none = cell.is_some_and(|c| c.model.is_none()) || role == "image";
        let current = cell.and_then(|c| c.model.clone());
        let effort = cell.map_or("medium", |c| c.effort.as_str()).to_owned();
        let mut out = Vec::new();
        for option in self.options.models(tool, allow_none) {
            let allowed = model_allowed(tool, floor, &option);
            let picked = match &current {
                Some(m) => *m == option.id,
                None => option.id == NO_MODEL && cell.is_some(),
            };
            let title = if option.id == NO_MODEL {
                t::costs::cell_no_model().to_owned()
            } else {
                option.id.clone()
            };
            let item = MenuItem::new(
                SharedString::from(format!("costs-model-{}", option.id)),
                title,
            )
            .checked(picked)
            .held(!allowed)
            .build(a);
            let item = if allowed {
                let text = choose_model(tool, floor, &option, &effort);
                let role = role.to_owned();
                item.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(text) = &text {
                        this.set_pick(&role, tool, text, cx);
                    }
                }))
            } else {
                match floor {
                    Some(f) => gate::tip_text(
                        item,
                        SharedString::from(t::costs::below_floor(&role_title(role), &f.text())),
                        a,
                    ),
                    None => item,
                }
            };
            out.push(item);
        }
        out
    }

    fn effort_buttons(
        &self,
        role: &str,
        tool: usize,
        cell: Option<&Cell>,
        floor: Option<&Floor>,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let model = cell.map(|c| c.model.clone().unwrap_or_else(|| NO_MODEL.to_owned()))?;
        let current = cell.map(|c| c.effort.clone())?;
        let allow_none = cell.is_some_and(|c| c.model.is_none());
        let option = self
            .options
            .models(tool, allow_none)
            .into_iter()
            .find(|o| o.id == model)?;
        let mut row = div().flex().flex_wrap().gap(px(4.0));
        for effort in &option.efforts {
            let allowed = effort_allowed(tool, floor, &model, effort);
            let kind = if *effort == current {
                ButtonKind::Secondary
            } else {
                ButtonKind::Ghost
            };
            let state = if allowed {
                PartState::Rest
            } else {
                PartState::Disabled
            };
            let id = SharedString::from(format!("costs-effort-{effort}"));
            let mut item = button(id, effort, kind, state, a);
            if allowed {
                let (role, text) = (role.to_owned(), pick_text(&model, effort));
                item = item.on_click(
                    cx.listener(move |this, _, _, cx| this.set_pick(&role, tool, &text, cx)),
                );
            } else if let Some(f) = floor {
                item = gate::tip_text(
                    item,
                    SharedString::from(t::costs::below_floor(&role_title(role), &f.text())),
                    a,
                );
            }
            row = row.child(item);
        }
        Some(row)
    }

    pub(super) fn picker_section(
        &self,
        profile: &Profile,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let (role, tool) = self.picker.clone()?;
        let row = profile.row(&role)?;
        let cell = row.cells[tool].as_ref();
        let floor = row.floor.as_ref();
        let close = icon_button(
            "costs-picker-close",
            Icon::Close,
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener({
            let role = role.clone();
            move |this, _, _, cx| this.open_picker(&role, tool, cx)
        }));
        let mut body = card(a).flex().flex_col().gap(px(8.0)).p(px(12.0)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    label(
                        &t::costs::picker_title(&role_title(&role), tool_title(tool)),
                        TypeRole::Heading,
                        Ink::Text,
                        a,
                    )
                    .flex_1(),
                )
                .child(close),
        );
        body = body
            .child(label(
                t::costs::picker_model(),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(self.model_rows(&role, tool, cell, floor, a, cx)),
            );
        if let Some(efforts) = self.effort_buttons(&role, tool, cell, floor, a, cx) {
            body = body
                .child(label(
                    t::costs::picker_effort(),
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                ))
                .child(efforts);
        }
        if cell.is_some_and(|c| c.own) {
            let role = role.clone();
            body = body.child(
                div().flex().child(
                    button(
                        "costs-picker-reset",
                        t::costs::picker_reset(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.clear_pick(&role, tool, cx))),
                ),
            );
        }
        if let Some(f) = floor.filter(|_| tool == 0) {
            body = body.child(label(
                &t::costs::picker_floor(&f.text()),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ));
        }
        Some(body)
    }

    pub(super) fn habits_section(&self, profile: &Profile, a: Appearance) -> Div {
        let lines = profile.habit_lines();
        let mut rows = card(a).flex().flex_col();
        for (i, line) in lines.iter().enumerate() {
            rows = rows.child(
                div()
                    .px(px(14.0))
                    .py(px(6.0))
                    .when(i > 0, |r| {
                        r.border_t(px(1.0))
                            .border_color(hsla(Ink::Hairline.color(a)))
                    })
                    .child(label(line, TypeRole::Body, Ink::Text, a)),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::costs::habits_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(rows)
    }

    pub(super) fn imported_section(&self, profile: &Profile, a: Appearance) -> Option<Div> {
        if profile.imports.is_empty() {
            return None;
        }
        let mut rows = card(a).flex().flex_col();
        for (i, imported) in profile.imports.iter().enumerate() {
            let moved = imported
                .lines
                .iter()
                .filter(|l| l.fate.starts_with("profile:"))
                .count();
            let kept = imported.lines.iter().filter(|l| l.fate == "kept").count();
            let source = short_path(&imported.source, &self.home);
            rows = rows.child(
                div()
                    .px(px(14.0))
                    .py(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .when(i > 0, |r| {
                        r.border_t(px(1.0))
                            .border_color(hsla(Ink::Hairline.color(a)))
                    })
                    .child(label(&source, TypeRole::Mono, Ink::Text, a))
                    .child(label(
                        &t::costs::imported_summary(moved as u64, kept as u64),
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    )),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(label(
                    t::costs::imported_title(),
                    TypeRole::Section,
                    Ink::TextFaint,
                    a,
                ))
                .child(rows),
        )
    }
}
