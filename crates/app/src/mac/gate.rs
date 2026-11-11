//! Action controls over GPUI: a control acting allows is live; one it doesn't is drawn disabled,
//! takes no click and says why on hover. The tooltip is a small kit card.

use gpui::{div, prelude::*, px, Context, Div, Render, SharedString, Stateful, Window};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::part::PartState;
use notchling_kit::ui::{hsla, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use crate::acting::Control;

/// A tooltip's view.
pub struct Tip {
    text: SharedString,
    /// A second, fainter line (a terminal's user, host and folder).
    detail: Option<SharedString>,
    appearance: Appearance,
}

impl Tip {
    pub fn new(text: SharedString, appearance: Appearance) -> Tip {
        Tip {
            text,
            detail: None,
            appearance,
        }
    }
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let a = self.appearance;
        div()
            .max_w(px(300.0))
            .px(px(8.0))
            .py(px(5.0))
            .rounded(px(RADIUS_SM))
            .fill(Ink::Card, a)
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .kit_type(TypeRole::Meta)
            .ink(Ink::Text, a)
            .child(self.text.clone())
            .children(
                self.detail
                    .clone()
                    .map(|d| div().ink(Ink::TextFaint, a).child(d)),
            )
    }
}

/// A tooltip of a title and a fainter detail line under it.
pub fn tip_detail(
    element: Stateful<Div>,
    text: SharedString,
    detail: SharedString,
    appearance: Appearance,
) -> Stateful<Div> {
    element.tooltip(move |_, cx| {
        let (text, detail) = (text.clone(), Some(detail.clone()));
        cx.new(|_| Tip {
            text,
            detail,
            appearance,
        })
        .into()
    })
}

/// The part state a control is drawn in.
pub fn state(control: Control) -> PartState {
    if control.enabled {
        PartState::Rest
    } else {
        PartState::Disabled
    }
}

/// Puts a tooltip made at render time (a meter's numbers) on an element.
pub fn tip_text(
    element: Stateful<Div>,
    text: SharedString,
    appearance: Appearance,
) -> Stateful<Div> {
    element.tooltip(move |_, cx| {
        let text = text.clone();
        cx.new(|_| Tip::new(text, appearance)).into()
    })
}

/// Puts a control's tooltip on an element.
pub fn tip(element: Stateful<Div>, control: Control, appearance: Appearance) -> Stateful<Div> {
    match control.tooltip {
        Some(text) => element.tooltip(move |_, cx| {
            cx.new(|_| Tip::new(SharedString::from(text), appearance))
                .into()
        }),
        None => element,
    }
}
