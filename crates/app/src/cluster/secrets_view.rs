use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::{SIDE_PADDING, SPACE};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, icon, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::model::{Login, NodeRow};
use super::secrets::{summary, SecretsModel, SecretsPicker, MAX_ATTACHED};
use crate::accounts::view_rows::Handler;
use notchling_engine::cluster::grants::MAX_ALLOWED_NAMES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretsEvent {
    Toggle(String),
    Any,
    Save,
    Cancel,
}

pub struct Editor<'a> {
    pub name: &'a str,
    pub model: Option<&'a SecretsModel>,
    pub error: Option<&'a str>,
    pub busy: bool,
}

fn login_ink(login: Login) -> Ink {
    match login {
        Login::SignedIn => Ink::Done,
        Login::SignedOut => Ink::Needs,
        Login::NotInstalled => Ink::TextFaint,
        Login::Unknown => Ink::TextMuted,
    }
}

pub fn login_line(row: &NodeRow, dim: Ink, a: Appearance) -> Option<Div> {
    let texts = row.login_texts();
    if texts.is_empty() {
        return None;
    }
    let mut line = div().flex().flex_wrap().items_center().gap_x(px(SPACE[1]));
    for (n, (text, login)) in texts.iter().enumerate() {
        if n > 0 {
            line = line.child(label("·", TypeRole::Meta, dim, a));
        }
        let ink = if dim == Ink::TextFaint && *login != Login::SignedOut {
            dim
        } else {
            login_ink(*login)
        };
        line = line.child(label(text, TypeRole::Meta, ink, a));
    }
    Some(line)
}

pub fn secrets_line(row: &NodeRow, dim: Ink, a: Appearance) -> Option<Div> {
    row.offers_secrets().then(|| {
        div()
            .flex()
            .items_center()
            .gap(px(SPACE[0]))
            .child(icon(Icon::Key, TypeRole::Meta.spec().size, dim, a))
            .child(label(&summary(&row.secrets), TypeRole::Meta, dim, a))
    })
}

fn checkbox(on: bool, a: Appearance) -> Div {
    div()
        .flex_shrink_0()
        .size(px(SPACE[3]))
        .rounded(px(SPACE[0]))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .fill(if on { Ink::Accent } else { Ink::Raised }, a)
        .flex()
        .items_center()
        .justify_center()
        .when(on, |m| {
            m.child(icon(
                Icon::Check,
                TypeRole::Meta.spec().size,
                Ink::OnAccent,
                a,
            ))
        })
}

struct Check<'a> {
    id: SharedString,
    on: bool,
    title: &'a str,
    detail: Option<&'a str>,
    event: SecretsEvent,
}

fn check_row<V: 'static>(
    check: Check,
    a: Appearance,
    cx: &mut Context<V>,
    handler: &Handler<V, SecretsEvent>,
) -> Stateful<Div> {
    let handler = handler.clone();
    let event = check.event;
    let mut text = div().flex_1().min_w(px(0.0)).flex().flex_col().child(label(
        check.title,
        TypeRole::Body,
        Ink::Text,
        a,
    ));
    if let Some(detail) = check.detail {
        text = text.child(label(detail, TypeRole::Meta, Ink::TextMuted, a));
    }
    div()
        .id(check.id)
        .flex()
        .items_start()
        .gap(px(SPACE[1]))
        .cursor_pointer()
        .child(checkbox(check.on, a))
        .child(text)
        .on_click(cx.listener(move |this, _, _, cx| handler(this, event.clone(), cx)))
}

pub fn editor_title(name: &str) -> String {
    t::cluster::secrets_title(name)
}

pub fn editor_body<V: 'static>(
    editor: &Editor,
    a: Appearance,
    cx: &mut Context<V>,
    on: &Handler<V, SecretsEvent>,
) -> Div {
    let mut body = div().flex().flex_col().gap(px(SPACE[1])).child(label(
        t::cluster::secrets_about(),
        TypeRole::Meta,
        Ink::TextMuted,
        a,
    ));
    let Some(model) = editor.model else {
        let (text, ink) = match editor.error {
            Some(error) => (t::cluster::secrets_vault_failed(error), Ink::Needs),
            None => (t::cluster::secrets_reading().to_owned(), Ink::TextMuted),
        };
        return body.child(label(&text, TypeRole::Body, ink, a));
    };
    let any = Check {
        id: SharedString::from("secrets-any"),
        on: model.any(),
        title: t::cluster::secrets_any(),
        detail: Some(t::cluster::secrets_any_about()),
        event: SecretsEvent::Any,
    };
    body = body.child(check_row(any, a, cx, on));
    if model.shows_list() {
        let mut list = card(a).p(px(SPACE[1])).flex().flex_col().gap(px(SPACE[1]));
        if model.rows().is_empty() {
            list = list.child(label(
                t::cluster::secrets_empty(),
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ));
        }
        for row in model.rows() {
            let detail = (!row.in_vault).then(t::cluster::secrets_gone);
            let check = Check {
                id: SharedString::from(format!("secret-{}", row.name)),
                on: row.on,
                title: &row.name,
                detail,
                event: SecretsEvent::Toggle(row.name.clone()),
            };
            list = list.child(check_row(check, a, cx, on));
        }
        body = body.child(list);
    }
    if model.at_cap() {
        body = body.child(label(
            &t::cluster::secrets_cap(&MAX_ALLOWED_NAMES.to_string()),
            TypeRole::Meta,
            Ink::Needs,
            a,
        ));
    }
    if let Some(error) = editor.error {
        body = body.child(label(
            &t::cluster::secrets_failed(error),
            TypeRole::Meta,
            Ink::Needs,
            a,
        ));
    }
    body.part("Node Secrets", a, cx)
}

pub fn editor_footer<V: 'static>(
    editor: &Editor,
    a: Appearance,
    cx: &mut Context<V>,
    on: &Handler<V, SecretsEvent>,
) -> Div {
    let can = editor.model.is_some_and(SecretsModel::is_changed) && !editor.busy;
    let (save_on, cancel_on) = (on.clone(), on.clone());
    let mut save = button(
        "secrets-save",
        t::cluster::secrets_save(),
        ButtonKind::Primary,
        if can {
            PartState::Rest
        } else {
            PartState::Disabled
        },
        a,
    );
    if can {
        save =
            save.on_click(cx.listener(move |this, _, _, cx| save_on(this, SecretsEvent::Save, cx)));
    }
    let cancel = button(
        "secrets-cancel",
        t::cluster::secrets_cancel(),
        ButtonKind::Secondary,
        PartState::Rest,
        a,
    )
    .on_click(cx.listener(move |this, _, _, cx| cancel_on(this, SecretsEvent::Cancel, cx)));
    div()
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .py(px(SPACE[2]))
        .flex()
        .items_center()
        .justify_end()
        .gap(px(SPACE[1]))
        .border_t(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .child(cancel)
        .child(save)
}

pub fn picker<V: 'static>(
    picker: &SecretsPicker,
    a: Appearance,
    cx: &mut Context<V>,
    on: &Handler<V, SecretsEvent>,
) -> Div {
    let mut out = div().flex().flex_col().gap(px(SPACE[0])).child(label(
        t::cluster::secrets_picker(),
        TypeRole::Label,
        Ink::TextFaint,
        a,
    ));
    if picker.is_empty() {
        out = out.child(label(
            t::cluster::secrets_picker_none(),
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ));
    }
    for name in picker.offered() {
        let check = Check {
            id: SharedString::from(format!("pick-secret-{name}")),
            on: picker.is_on(name),
            title: name,
            detail: None,
            event: SecretsEvent::Toggle(name.clone()),
        };
        out = out.child(check_row(check, a, cx, on));
    }
    if picker.at_cap() {
        out = out.child(label(
            &t::cluster::secrets_cap(&MAX_ATTACHED.to_string()),
            TypeRole::Meta,
            Ink::Needs,
            a,
        ));
    }
    if let Some(note) = picker.note() {
        out = out.child(label(note, TypeRole::Meta, Ink::Needs, a));
    }
    out.part("Node Secrets", a, cx)
}

pub fn vault_tabs<V: 'static>(
    name: Option<&str>,
    remote: bool,
    a: Appearance,
    cx: &mut Context<V>,
    on: Handler<V, bool>,
) -> Div {
    let kind = |active: bool| {
        if active {
            ButtonKind::Secondary
        } else {
            ButtonKind::Ghost
        }
    };
    let (here, there) = (on.clone(), on);
    div()
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .py(px(SPACE[1]))
        .flex()
        .items_center()
        .gap(px(SPACE[0]))
        .child(
            button(
                "vault-tab-local",
                t::cluster::vault_tab_local(),
                kind(!remote),
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| here(this, false, cx))),
        )
        .child(
            button(
                "vault-tab-remote",
                &name.map_or_else(
                    || t::cluster::vault_tab_unknown().to_owned(),
                    t::cluster::vault_tab_remote,
                ),
                kind(remote),
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| there(this, true, cx))),
        )
}
