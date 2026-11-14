use gpui::{div, prelude::*, px, Div, SharedString, Stateful};

use super::{card, hsla, icon, label, KitStyled};
use crate::metric::{
    MENU_GAP, MENU_MARK, MENU_MAX_WIDTH, MENU_MIN_WIDTH, MENU_PADDING, MENU_ROW_GAP, MENU_ROW_X,
    MENU_ROW_Y, MENU_RULE_Y, RADIUS_SM,
};
use crate::{Appearance, Icon, Ink, TypeRole};

pub fn menu_panel(a: Appearance) -> Div {
    card(a)
        .min_w(px(MENU_MIN_WIDTH))
        .max_w(px(MENU_MAX_WIDTH))
        .p(px(MENU_PADDING))
        .flex()
        .flex_col()
        .shadow_lg()
        .occlude()
}

pub fn menu_panel_wide(a: Appearance) -> Div {
    menu_panel(a).w(px(MENU_MAX_WIDTH))
}

pub fn menu_rule(a: Appearance) -> Div {
    div()
        .flex_shrink_0()
        .py(px(MENU_RULE_Y))
        .child(div().h(px(1.0)).bg(hsla(Ink::Hairline.color(a))))
}

pub fn menu_section(text: &str, a: Appearance) -> Div {
    label(text, TypeRole::Section, Ink::TextFaint, a)
        .px(px(MENU_ROW_X))
        .pt(px(MENU_ROW_Y))
}

pub fn menu_note(text: &str, a: Appearance) -> Div {
    label(text, TypeRole::Meta, Ink::TextFaint, a)
        .px(px(MENU_ROW_X))
        .py(px(MENU_ROW_Y))
}

pub fn menu_row(id: impl Into<SharedString>, a: Appearance) -> Stateful<Div> {
    row_base(id.into())
        .cursor_pointer()
        .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
}

fn row_base(id: SharedString) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .px(px(MENU_ROW_X))
        .py(px(MENU_ROW_Y))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(MENU_ROW_GAP))
}

pub struct MenuItem {
    id: SharedString,
    title: SharedString,
    detail: Option<SharedString>,
    inline: bool,
    leading: Option<Icon>,
    shortcut: Option<SharedString>,
    checked: Option<bool>,
    chevron: Option<Icon>,
    held: bool,
}

impl MenuItem {
    pub fn new(id: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        MenuItem {
            id: id.into(),
            title: title.into(),
            detail: None,
            inline: false,
            leading: None,
            shortcut: None,
            checked: None,
            chevron: None,
            held: false,
        }
    }

    pub fn detail(mut self, detail: &str) -> Self {
        if !detail.is_empty() {
            self.detail = Some(detail.to_owned().into());
        }
        self
    }

    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.leading = Some(icon);
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn chevron(mut self, chevron: Icon) -> Self {
        self.chevron = Some(chevron);
        self
    }

    pub fn held(mut self, held: bool) -> Self {
        self.held = held;
        self
    }

    pub fn build(self, a: Appearance) -> Stateful<Div> {
        let (title_ink, detail_ink) = if self.held {
            (Ink::TextFaint, Ink::TextFaint)
        } else {
            (Ink::Text, Ink::TextFaint)
        };
        let meta = TypeRole::Meta.spec().size;
        let row = if self.held {
            row_base(self.id)
        } else {
            menu_row(self.id, a)
        };
        let leading = self
            .leading
            .map(|glyph| icon(glyph, crate::metric::MENU_ICON, Ink::TextMuted, a).flex_shrink_0());
        let title = label(&self.title, TypeRole::Meta, title_ink, a).whitespace_nowrap();
        let text = match (&self.detail, self.inline) {
            (Some(detail), true) => div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .items_baseline()
                .gap(px(MENU_GAP))
                .child(title.flex_shrink_0())
                .child(
                    label(detail, TypeRole::Meta, detail_ink, a)
                        .min_w(px(0.0))
                        .whitespace_nowrap()
                        .elide(),
                ),
            (Some(detail), false) => div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(title.elide())
                .child(label(detail, TypeRole::Meta, detail_ink, a).line_clamp(2)),
            (None, _) => div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(title.elide()),
        };
        let end = match (self.chevron, self.checked) {
            (Some(glyph), _) => Some(div().w(px(MENU_MARK)).flex_shrink_0().child(icon(
                glyph,
                9.0,
                Ink::TextMuted,
                a,
            ))),
            (None, Some(on)) => Some(
                div()
                    .w(px(MENU_MARK))
                    .flex_shrink_0()
                    .children(on.then(|| icon(Icon::Check, meta, Ink::AccentText, a))),
            ),
            (None, None) => None,
        };
        row.children(leading)
            .child(text)
            .children(
                self.shortcut
                    .map(|k| label(&k, TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0()),
            )
            .children(end)
    }
}
