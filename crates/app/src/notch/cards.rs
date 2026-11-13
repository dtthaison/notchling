use gpui::{div, prelude::*, px, Div, SharedString};
use notchling_engine::transcript::snapshot::{ChatLine, LineKind};
use notchling_kit::metric::{RADIUS_MD, RADIUS_SM};
use notchling_kit::ui::{hsla, icon, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

pub const A: Appearance = Appearance::Dark;
pub const LINE: f32 = 1.3;

pub fn line(role: TypeRole) -> f32 {
    role.spec().size * LINE
}

pub fn text(content: impl Into<SharedString>, role: TypeRole, ink: Ink) -> Div {
    div()
        .kit_type(role)
        .ink(ink, A)
        .line_height(px(line(role)))
        .child(role.display(&content.into()))
}

pub fn tag(host: &str) -> Div {
    text(host.to_owned(), TypeRole::Meta, Ink::TextMuted)
        .flex_shrink_0()
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, A)
}

pub fn bubble(entry: &ChatLine) -> Div {
    let (mark, mark_ink, ink) = match entry.kind {
        LineKind::User => ("❯", Ink::TextMuted, Ink::Text),
        LineKind::Assistant => ("⏺", Ink::Text, Ink::Text),
        LineKind::Tools => ("⎿", Ink::TextFaint, Ink::TextMuted),
    };
    let row = div()
        .flex()
        .items_start()
        .gap(px(6.0))
        .child(text(mark, TypeRole::Mono, mark_ink).flex_shrink_0())
        .child(
            text(entry.text.clone(), TypeRole::Mono, ink)
                .flex_1()
                .min_w(px(0.0)),
        );
    match entry.kind {
        LineKind::User => row
            .px(px(6.0))
            .py(px(5.0))
            .rounded(px(4.0))
            .fill(Ink::Raised, A),
        LineKind::Assistant => row.px(px(6.0)),
        LineKind::Tools => row.pl(px(18.0)).pr(px(6.0)),
    }
}

pub fn needs_box(needs: &str) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(8.0))
        .p(px(10.0))
        .rounded(px(RADIUS_MD))
        .bg(hsla(Ink::Needs.color(A).with_alpha(0.15)))
        .child(icon(Icon::Needs, TypeRole::Body.spec().size, Ink::Needs, A))
        .child(
            text(needs.to_owned(), TypeRole::Body, Ink::Text)
                .flex_1()
                .min_w(px(0.0)),
        )
}
