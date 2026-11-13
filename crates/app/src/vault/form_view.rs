use std::rc::Rc;

use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_engine::vault::entry::{Field as Changed, Kind};
use notchling_kit::metric::{RADIUS_MD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, field, hsla, icon, label, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::form::{Field, Form, Mode};
use super::list::expiry_label;
use crate::accounts::view_rows::{segmented, Handler};
use crate::mac::gate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormEvent {
    Focus(Field),
    Kind(Kind),
    ToggleAsk,
    ToggleAdvanced,
    EditHowTo,
    NoExpiry(bool),
    ConfirmHost,
    Save,
    Cancel,
}

pub fn clipboard_key<V: 'static>(
    form: &mut Form,
    k: &gpui::Keystroke,
    cx: &mut Context<V>,
) -> bool {
    if !k.modifiers.platform || k.modifiers.control || k.modifiers.alt {
        return false;
    }
    match k.key.as_str() {
        "v" => {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                form.paste(&text);
            }
        }
        "c" => {
            if let Some(text) = form.copy() {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
        }
        "x" => {
            if let Some(text) = form.cut() {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
        }
        "a" => {}
        _ => return false,
    }
    true
}

pub const FOOTER: &str =
    "Agents see the name, URL, variables, note, how-to and expiry. Only Notchling can read the value.";
const HOW_TO_PLACEHOLDER: &str = "Where to click and which scopes, e.g. GitHub > Settings > Developer settings > Tokens. Links work (https only).";
const NOTE_PLACEHOLDER: &str = "What it's for, e.g. GitHub API for my repos";
const VARS_TIP: &str = "The environment variable names a command gets. Made from the name; change them only when a tool expects another name.";

fn row_label(text: &str, a: Appearance) -> Div {
    div()
        .w(px(92.0))
        .flex_shrink_0()
        .pt(px(6.0))
        .flex()
        .justify_end()
        .child(label(text, TypeRole::Meta, Ink::TextMuted, a))
}

fn row(title: &str, body: impl IntoElement, a: Appearance) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(10.0))
        .child(row_label(title, a))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(body),
        )
}

fn text_field<V: 'static>(
    form: &Form,
    which: Field,
    placeholder: &str,
    a: Appearance,
    cx: &mut Context<V>,
    on: &Handler<V, FormEvent>,
) -> Stateful<Div> {
    let text = if which == Field::Secret {
        form.secret_mask()
    } else {
        form.text(which).to_owned()
    };
    let focused = form.focus == which;
    let handler = on.clone();
    let mut element = field(&text, placeholder, focused, a).w_full();
    if which.multiline() {
        element = element.min_h(px(64.0)).items_start().py(px(6.0));
    }
    div()
        .id(SharedString::from(format!("vault-field-{which:?}")))
        .w_full()
        .cursor_text()
        .child(element)
        .on_click(cx.listener(move |this, _, _, cx| handler(this, FormEvent::Focus(which), cx)))
}

fn marker(form: &Form, changed: Changed, a: Appearance) -> Option<Div> {
    form.marker(changed).map(|m| {
        div()
            .flex()
            .flex_col()
            .gap(px(1.0))
            .child(label(m.text, TypeRole::Meta, Ink::AccentText, a))
            .child(label(&m.was, TypeRole::Meta, Ink::TextFaint, a))
    })
}

fn checkbox<V: 'static>(
    id: &str,
    title: &str,
    on_now: bool,
    a: Appearance,
    cx: &mut Context<V>,
    event: FormEvent,
    on: &Handler<V, FormEvent>,
) -> Stateful<Div> {
    let handler = on.clone();
    let mark = div()
        .size(px(16.0))
        .rounded(px(4.0))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .fill(if on_now { Ink::Accent } else { Ink::Raised }, a)
        .flex()
        .items_center()
        .justify_center()
        .when(on_now, |m| {
            m.child(icon(
                Icon::Check,
                TypeRole::Meta.spec().size,
                Ink::OnAccent,
                a,
            ))
        });
    div()
        .id(SharedString::from(id.to_owned()))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .child(mark)
        .child(label(title, TypeRole::Body, Ink::Text, a))
        .on_click(cx.listener(move |this, _, _, cx| handler(this, event.clone(), cx)))
}

fn help_box<V: 'static>(
    form: &Form,
    a: Appearance,
    cx: &mut Context<V>,
    on: &Handler<V, FormEvent>,
) -> Div {
    let handler = on.clone();
    div()
        .p(px(10.0))
        .rounded(px(RADIUS_MD))
        .fill(Ink::AccentWash, a)
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .child(label("How to get or renew", TypeRole::Section, Ink::TextFaint, a).flex_1())
                .child(
                    button("how-to-edit", "Edit", ButtonKind::Ghost, PartState::Rest, a).on_click(
                        cx.listener(move |this, _, _, cx| handler(this, FormEvent::EditHowTo, cx)),
                    ),
                ),
        )
        .child(label(&form.how_to, TypeRole::Body, Ink::Text, a))
}

fn expiry_row<V: 'static>(
    form: &Form,
    today: i64,
    a: Appearance,
    cx: &mut Context<V>,
    on: &Handler<V, FormEvent>,
) -> Div {
    let body = if !form.expiry_chosen {
        let (pick, none) = (on.clone(), on.clone());
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                button(
                    "pick-date",
                    "Pick a date",
                    ButtonKind::Secondary,
                    PartState::Rest,
                    a,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| pick(this, FormEvent::NoExpiry(false), cx)),
                ),
            )
            .child(
                button(
                    "no-expiry",
                    "No expiry",
                    ButtonKind::Secondary,
                    PartState::Rest,
                    a,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| none(this, FormEvent::NoExpiry(true), cx)),
                ),
            )
            .child(label(
                "Choose one to save",
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
    } else {
        let mut body = div().flex().flex_col().gap(px(6.0)).child(checkbox(
            "no-expiry-box",
            "No expiry",
            form.no_expiry,
            a,
            cx,
            FormEvent::NoExpiry(!form.no_expiry),
            on,
        ));
        if !form.no_expiry {
            let shown = expiry_label(&form.expires, today);
            body = body
                .child(text_field(form, Field::Expires, "YYYY-MM-DD", a, cx, on))
                .child(label(
                    &shown.text,
                    TypeRole::Meta,
                    if shown.expired {
                        Ink::Expired
                    } else {
                        Ink::TextFaint
                    },
                    a,
                ));
        }
        body
    };
    row(
        form.expires_label(),
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(body)
            .children(marker(form, Changed::Expires, a)),
        a,
    )
}

pub fn form_body<V: 'static>(
    form: &Form,
    today: i64,
    a: Appearance,
    cx: &mut Context<V>,
    on: Handler<V, FormEvent>,
) -> Stateful<Div> {
    let mut body = div()
        .id("vault-form")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .p(px(SIDE_PADDING))
        .flex()
        .flex_col()
        .gap(px(12.0));
    if let Some(reason) = form.reason_line() {
        body = body.child(
            div()
                .ml(px(102.0))
                .p(px(10.0))
                .rounded(px(RADIUS_MD))
                .fill(Ink::AccentWash, a)
                .flex()
                .gap(px(8.0))
                .child(icon(
                    Icon::Claude,
                    TypeRole::Body.spec().size,
                    Ink::AccentText,
                    a,
                ))
                .child(label(&reason, TypeRole::Body, Ink::Text, a)),
        );
    }
    body = body.child(row(
        "Name",
        text_field(form, Field::Name, "github", a, cx, &on),
        a,
    ));
    let mut url = div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(text_field(
            form,
            Field::Url,
            "https://api.github.com",
            a,
            cx,
            &on,
        ))
        .children(marker(form, Changed::Url, a));
    if let Some(warning) = form.host_warning() {
        url = url.child(label(&warning, TypeRole::Meta, Ink::Needs, a));
        if form.mode == Mode::Request {
            url = url.child(checkbox(
                "confirm-host",
                "I checked the new address; send the secret there",
                form.host_confirmed,
                a,
                cx,
                FormEvent::ConfirmHost,
                &on,
            ));
        }
    }
    if let Some(note) = form.renew_note() {
        url = url.child(label(&note, TypeRole::Meta, Ink::TextMuted, a));
    }
    body = body.child(row("URL", url, a));
    let kinds = vec![
        ("token".to_owned(), "Token".to_owned()),
        ("login".to_owned(), "Username & Password".to_owned()),
    ];
    let pick_kind: Handler<V, String> = {
        let on = on.clone();
        Rc::new(move |this, key: String, cx| {
            if let Some(kind) = Kind::parse(&key) {
                on(this, FormEvent::Kind(kind), cx);
            }
        })
    };
    body = body.child(row(
        "Kind",
        div().flex().child(segmented(
            "vault-kind",
            &kinds,
            form.kind.as_str(),
            a,
            cx,
            pick_kind,
        )),
        a,
    ));
    if form.is_login() {
        body = body.child(row(
            "Username",
            text_field(form, Field::Username, "", a, cx, &on),
            a,
        ));
    }
    let has_how_to = !form.how_to.trim().is_empty();
    if form.mode == Mode::Renew && has_how_to && !form.editing_how_to {
        body = body.child(row("", help_box(form, a, cx, &on), a));
    }
    let secret_title = if form.is_login() { "Password" } else { "Token" };
    body = body.child(row(
        secret_title,
        text_field(form, Field::Secret, form.secret_placeholder(), a, cx, &on),
        a,
    ));
    if form.mode != Mode::Renew && has_how_to && !form.editing_how_to {
        body = body.child(row("", help_box(form, a, cx, &on), a));
    }
    body = body.child(row(
        "Note",
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(text_field(form, Field::Note, NOTE_PLACEHOLDER, a, cx, &on))
            .children(marker(form, Changed::Note, a)),
        a,
    ));
    if form.editing_how_to {
        body = body.child(row(
            "How to get or renew",
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(text_field(
                    form,
                    Field::HowTo,
                    HOW_TO_PLACEHOLDER,
                    a,
                    cx,
                    &on,
                ))
                .children(marker(form, Changed::HowTo, a)),
            a,
        ));
    }
    body = body.child(expiry_row(form, today, a, cx, &on));
    body = body.child(row(
        "",
        checkbox(
            "ask-first",
            "Ask me before an agent uses it",
            form.ask,
            a,
            cx,
            FormEvent::ToggleAsk,
            &on,
        ),
        a,
    ));
    let toggle = on.clone();
    let advanced = div()
        .id("advanced")
        .flex()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .child(icon(
            if form.advanced {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            9.0,
            Ink::TextMuted,
            a,
        ))
        .child(label("Advanced", TypeRole::Body, Ink::Text, a))
        .when(!form.advanced, |d| {
            d.child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(px(11.0))
                    .ink(Ink::TextFaint, a)
                    .child(form.vars_summary()),
            )
        })
        .on_click(cx.listener(move |this, _, _, cx| toggle(this, FormEvent::ToggleAdvanced, cx)));
    body = body.child(row("", gate::tip_text(advanced, VARS_TIP.into(), a), a));
    if form.advanced {
        if form.is_login() {
            body = body.child(row(
                "Username var",
                text_field(form, Field::UserVar, "", a, cx, &on),
                a,
            ));
        }
        body = body.child(row(
            "Secret var",
            text_field(form, Field::SecretVar, "", a, cx, &on),
            a,
        ));
    }
    body
}

pub fn form_footer<V: 'static>(
    form: &Form,
    busy: bool,
    off: Option<&'static str>,
    a: Appearance,
    cx: &mut Context<V>,
    on: Handler<V, FormEvent>,
) -> Div {
    let control = crate::acting::Control {
        enabled: off.is_none() && form.can_save() && !busy,
        tooltip: off,
    };
    let (save_on, cancel_on) = (on.clone(), on);
    let save = gate::tip(
        button(
            "vault-save",
            "Save",
            ButtonKind::Primary,
            gate::state(control),
            a,
        ),
        control,
        a,
    )
    .when(control.enabled, |b| {
        b.on_click(cx.listener(move |this, _, _, cx| save_on(this, FormEvent::Save, cx)))
    });
    let cancel = button(
        "vault-cancel",
        "Cancel",
        ButtonKind::Secondary,
        PartState::Rest,
        a,
    )
    .on_click(cx.listener(move |this, _, _, cx| cancel_on(this, FormEvent::Cancel, cx)));
    let problem = form.error.as_deref().or(form.blocker());
    div()
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .border_t(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .child(label(FOOTER, TypeRole::Meta, Ink::TextFaint, a))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    label(problem.unwrap_or(""), TypeRole::Meta, Ink::Needs, a)
                        .flex_1()
                        .min_w(px(0.0)),
                )
                .child(cancel)
                .child(save),
        )
}
