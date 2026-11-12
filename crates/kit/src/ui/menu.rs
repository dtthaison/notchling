//! The popup menu: one look for every list of actions or choices in a floating panel, taken from
//! Chat's model picker. A [`menu_panel`] hugs its content (between `MENU_MIN_WIDTH` and
//! `MENU_MAX_WIDTH`), its rows are [`MenuItem`]s in the small Meta type with tight padding, a
//! check or chevron at the right end and a Raised wash under the pointer; [`menu_rule`],
//! [`menu_section`] and [`menu_note`] are the divider and the small texts between rows. Views
//! never size a menu themselves.

use gpui::{div, prelude::*, px, Div, SharedString, Stateful};

use super::{card, hsla, icon, label, KitStyled};
use crate::metric::{
    MENU_MARK, MENU_MAX_WIDTH, MENU_MIN_WIDTH, MENU_PADDING, MENU_ROW_GAP, MENU_ROW_X, MENU_ROW_Y,
    MENU_RULE_Y, RADIUS_SM,
};
use crate::{Appearance, Icon, Ink, TypeRole};

/// The panel: the card, tight padding, rows stacked with no gap, a shadow, sized to its content.
/// The caller adds the rows, an id and `overflow_y_scroll` when it can run tall.
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

/// A panel for a menu whose rows wrap: a note, or a detail under a title. Text that wraps needs
/// the width it wraps in before it is laid out, which a panel hugging its content doesn't give,
/// so this one is `MENU_MAX_WIDTH` wide.
pub fn menu_panel_wide(a: Appearance) -> Div {
    menu_panel(a).w(px(MENU_MAX_WIDTH))
}

/// The thin divider between groups of rows.
pub fn menu_rule(a: Appearance) -> Div {
    // No `w_full`: the panel hugs its content, so a percentage of its width would be nothing; a
    // column's children stretch on their own.
    div()
        .flex_shrink_0()
        .py(px(MENU_RULE_Y))
        .child(div().h(px(1.0)).bg(hsla(Ink::Hairline.color(a))))
}

/// A small heading over a group of rows.
pub fn menu_section(text: &str, a: Appearance) -> Div {
    label(text, TypeRole::Section, Ink::TextFaint, a)
        .px(px(MENU_ROW_X))
        .pt(px(MENU_ROW_Y))
}

/// A faint sentence under the rows.
pub fn menu_note(text: &str, a: Appearance) -> Div {
    label(text, TypeRole::Meta, Ink::TextFaint, a)
        .px(px(MENU_ROW_X))
        .py(px(MENU_ROW_Y))
}

/// The bare row: padding, corner, the Raised wash under the pointer and the pointing hand. For a
/// row with its own contents (the command list); [`MenuItem`] is the usual one.
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

/// One row of a menu. `checked(..)` reserves the check's slot at the right end (ticked or not, so
/// rows line up); `held()` draws it faint, with no hover and no pointing hand, for a row that is
/// off (the caller adds the tooltip and no click).
pub struct MenuItem {
    id: SharedString,
    title: SharedString,
    detail: Option<SharedString>,
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
            leading: None,
            shortcut: None,
            checked: None,
            chevron: None,
            held: false,
        }
    }

    /// A faint line under the title.
    pub fn detail(mut self, detail: &str) -> Self {
        if !detail.is_empty() {
            self.detail = Some(detail.to_owned().into());
        }
        self
    }

    /// An icon before the title.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.leading = Some(icon);
        self
    }

    /// A key hint at the right.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// A chevron at the right end: a row that opens more (a submenu or the rows under it).
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
        let mut text = div().flex_1().min_w(px(0.0)).flex().flex_col().child(
            label(&self.title, TypeRole::Meta, title_ink, a)
                .whitespace_nowrap()
                .elide(),
        );
        if let Some(detail) = &self.detail {
            text = text.child(label(detail, TypeRole::Meta, detail_ink, a).line_clamp(2));
        }
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
