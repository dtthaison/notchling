use gpui::{div, linear_color_stop, linear_gradient, prelude::*, px, Div};

use super::{hsla, KitStyled};
use crate::metric::{CODE_LINE, CODE_PAD_X, CODE_PAD_Y, RADIUS_SM};
use crate::{Appearance, Ink, TypeRole};

pub fn code_size() -> f32 {
    TypeRole::Meta.spec().size
}

pub fn code_line() -> f32 {
    code_size() * CODE_LINE
}

pub fn code_block(text: &str, max_lines: Option<usize>, fade: bool, a: Appearance) -> Div {
    let wash = Ink::Raised.color(a);
    let body = div()
        .kit_type(TypeRole::Mono)
        .text_size(px(code_size()))
        .line_height(px(code_line()))
        .ink(Ink::Text, a)
        .child(text.to_owned());
    let body = match max_lines {
        Some(lines) => div()
            .relative()
            .max_h(px(code_line() * lines as f32))
            .overflow_hidden()
            .child(body)
            .when(fade, |clip| {
                clip.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(code_line() * 1.5))
                        .bg(linear_gradient(
                            180.0,
                            linear_color_stop(hsla(wash.with_alpha(0.0)), 0.0),
                            linear_color_stop(hsla(wash), 1.0),
                        )),
                )
            }),
        None => div().child(body),
    };
    div()
        .w_full()
        .px(px(CODE_PAD_X))
        .py(px(CODE_PAD_Y))
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, a)
        .child(body)
}
