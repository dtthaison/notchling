use gpui::{div, prelude::*, px, AnyElement, Context, Div, SharedString, Stateful, Window};
use notchling_kit::metric::{
    OFFLINE_OPACITY, RADIUS_MD, SESSION_LINE_HEADING, SESSION_LINE_META, SESSION_ROW,
};
use notchling_kit::scrollbar::GUTTER;
use notchling_kit::ui::{hsla, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::run_on::{Code, Remote, RunOnRow};
use super::view::{choice_row, column_title, NewSessionView};
use crate::cluster::view::field_box;

const NARROW: f32 = 170.0;

fn run_on_row(id: SharedString, row: &RunOnRow, picked: bool, a: Appearance) -> Stateful<Div> {
    let (note, ink) = match (&row.reason, &row.warning) {
        (Some(reason), _) => (reason.clone(), Ink::Needs),
        (None, Some(warning)) => (warning.clone(), Ink::TextMuted),
        (None, None) if row.this_mac => (t::cluster::this_mac_detail().to_owned(), Ink::TextFaint),
        (None, None) => (t::cluster::online().to_owned(), Ink::TextFaint),
    };
    pick_row(id, &row.name, &note, ink, picked, row.pickable(), a)
}

fn pick_row(
    id: SharedString,
    title: &str,
    note: &str,
    ink: Ink,
    picked: bool,
    pickable: bool,
    a: Appearance,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .flex_shrink_0()
        .min_h(px(SESSION_ROW))
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(RADIUS_MD))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.0))
        .when(pickable, |r| r.cursor_pointer())
        .when(!pickable, |r| r.opacity(OFFLINE_OPACITY))
        .when(picked, |r| r.fill(Ink::Selection, a))
        .when(!picked && pickable, |r| {
            r.hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
        })
        .child(
            label(title, TypeRole::Label, Ink::Text, a)
                .line_height(px(SESSION_LINE_HEADING))
                .elide(),
        )
        .child(
            label(note, TypeRole::Meta, ink, a)
                .line_height(px(SESSION_LINE_META))
                .line_clamp(4),
        )
}

pub(super) fn run_on_column(
    view: &NewSessionView,
    a: Appearance,
    cx: &mut Context<NewSessionView>,
) -> Stateful<Div> {
    let me = view.model.run_on_rows();
    let picked = view
        .model
        .target
        .node
        .clone()
        .or_else(|| me.first().map(|r| r.id.clone()));
    let rows: Vec<AnyElement> = me
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let id = row.id.clone();
            let el = run_on_row(
                SharedString::from(format!("run-on-{i}")),
                row,
                picked.as_deref() == Some(row.id.as_str()),
                a,
            );
            let el = if row.pickable() {
                el.on_click(cx.listener(move |this, _, _, cx| {
                    this.model.pick_node(&id);
                    cx.notify();
                }))
            } else {
                el
            };
            el.into_any_element()
        })
        .collect();
    div()
        .id("run-on")
        .w(px(NARROW))
        .flex_shrink_0()
        .h_full()
        .overflow_y_scroll()
        .pl(px(8.0))
        .pr(px(GUTTER))
        .pt(px(4.0))
        .pb(px(12.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .border_r(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .child(column_title(t::cluster::run_on(), a))
        .children(rows)
        .part("Run On", a, cx)
}

pub(super) fn tool_column(
    view: &NewSessionView,
    a: Appearance,
    cx: &mut Context<NewSessionView>,
) -> Stateful<Div> {
    let name = view.model.node_row().map(|r| r.name).unwrap_or_default();
    let node = view.model.target.node.clone();
    let rows: Vec<AnyElement> = Remote::ALL
        .iter()
        .map(|remote| {
            let remote = *remote;
            let refusal = node
                .as_deref()
                .and_then(|id| view.model.run_on.row_of(id, remote.tool()))
                .and_then(|row| row.reason);
            let (note, ink) = match &refusal {
                Some(reason) => (reason.clone(), Ink::Needs),
                None if remote == Remote::Shell => {
                    (t::cluster::shell_detail(&name), Ink::TextFaint)
                }
                None => (t::cluster::uses_login(&name), Ink::TextFaint),
            };
            let row = pick_row(
                SharedString::from(format!("remote-{}", remote.word())),
                remote.title(),
                &note,
                ink,
                view.model.target.remote == remote,
                refusal.is_none(),
                a,
            );
            if refusal.is_some() {
                return row.into_any_element();
            }
            row.on_click(cx.listener(move |this, _, _, cx| {
                this.model.pick_remote(remote);
                cx.notify();
            }))
            .into_any_element()
        })
        .collect();
    div()
        .id("remote-tools")
        .w(px(230.0))
        .flex_shrink_0()
        .h_full()
        .overflow_y_scroll()
        .pl(px(8.0))
        .pr(px(GUTTER))
        .pt(px(4.0))
        .pb(px(12.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .border_r(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .child(column_title(t::cluster::run_on_tool(), a))
        .children(rows)
}

pub(super) fn code_column(
    view: &NewSessionView,
    folder_rows: Vec<AnyElement>,
    a: Appearance,
    window: &Window,
    cx: &mut Context<NewSessionView>,
) -> Stateful<Div> {
    let model = &view.model;
    let name = model.node_row().map(|r| r.name).unwrap_or_default();
    let code = |id: &'static str, title: &str, detail: &str, which: Code| {
        choice_row(id, title, detail, model.target.code == which, a).on_click(cx.listener(
            move |this, _, _, cx| {
                this.model.pick_code(which);
                cx.notify();
            },
        ))
    };
    let mut column = div()
        .id("remote-code")
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .overflow_y_scroll()
        .pl(px(8.0))
        .pr(px(GUTTER))
        .pt(px(4.0))
        .pb(px(12.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(column_title(t::cluster::code_title(), a))
        .child(code(
            "code-folder",
            t::cluster::code_folder(),
            &t::cluster::code_folder_detail(&name),
            Code::Folder,
        ))
        .child(code(
            "code-send",
            t::cluster::code_send(),
            &t::cluster::code_send_detail(&name),
            Code::Send,
        ));
    column = match model.target.code {
        Code::Send => column
            .child(column_title(t::chat::sheet_folder(), a))
            .children(folder_rows),
        Code::Folder => column
            .child(column_title(&t::cluster::folder_there(&name), a))
            .child(field_box(
                &view.path,
                view.path.read(cx).is_focused(window),
                a,
            )),
    };
    if model.target.remote.takes_text() {
        let hint = model.target.remote.text_hint();
        view.text
            .update(cx, |editor, cx| editor.set_placeholder(hint, cx));
        column = column
            .child(column_title(model.target.remote.text_title(), a))
            .child(field_box(
                &view.text,
                view.text.read(cx).is_focused(window),
                a,
            ));
    }
    column
}
