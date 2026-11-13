use gpui::{div, prelude::*, px, AnyElement, App, Context, Div, ExternalPaths, SharedString};
use notchling_kit::labels::Corner;
use notchling_kit::metric::TITLE_BAR;
use notchling_kit::metric::{SPACE, TOOL_ICON};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::scrollbar::GUTTER;
use notchling_kit::ui::{
    account_line, hsla, icon_button, label, parts_button, parts_on, parts_tip, scrollable,
    session_row_marked, toggle_parts, tool_icon, KitPart, KitStyled, ToolMark,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::ChatView;
use crate::chat::data::Tool;
use crate::chat::sidebar::{sidebar_model, SidebarModel};
use crate::mac::gate;

fn footer_button(
    id: &'static str,
    mark: Icon,
    title: &str,
    help: &str,
    a: Appearance,
    cx: &Context<ChatView>,
) -> gpui::Stateful<Div> {
    let button = icon_button(
        id,
        mark,
        IconSize::Button.points(),
        ButtonKind::Ghost,
        PartState::Rest,
        a,
    );
    gate::tip_text(button, help.to_owned().into(), a).part_at(
        &format!("{title} Button"),
        Corner::TopLeading,
        a,
        cx,
    )
}

pub(super) const FOOTER_HEIGHT: f32 = 44.0;

fn inbox_button(unread: usize, a: Appearance, cx: &Context<ChatView>) -> Div {
    let button = footer_button(
        "inbox",
        Icon::Bell,
        "Inbox",
        "Notifications that need you",
        a,
        cx,
    )
    .on_click(cx.listener(|this, _, _, cx| this.toggle_inbox(cx)));
    let badge = (unread > 0).then(|| {
        let text = if unread > 9 {
            "9+".to_owned()
        } else {
            unread.to_string()
        };
        div()
            .absolute()
            .top(px(0.0))
            .right(px(0.0))
            .min_w(px(SPACE[3]))
            .h(px(SPACE[3]))
            .px(px(SPACE[0] / 2.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(SPACE[3]))
            .fill(Ink::Accent, a)
            .child(label(&text, TypeRole::Meta, Ink::OnAccent, a))
    });
    div().relative().child(button).children(badge)
}

fn footer(
    a: Appearance,
    note: Option<&'static str>,
    unread: usize,
    cx: &mut Context<ChatView>,
) -> Div {
    let on = parts_on(cx);
    let parts = gate::tip_text(parts_button("parts", a, cx), parts_tip(on).into(), a)
        .on_click(|_, _, cx| {
            toggle_parts(cx);
        })
        .part_at("Parts Button", Corner::TopLeading, a, cx);
    div()
        .min_h(px(FOOTER_HEIGHT))
        .flex_shrink_0()
        .px(px(12.0))
        .py(px(6.0))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(
            footer_button(
                "settings",
                Icon::Settings,
                "Settings",
                "Settings: accounts, token cost and the app (⌘,)",
                a,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx))),
        )
        .child(
            footer_button(
                "accounts",
                Icon::Account,
                "Accounts",
                "Accounts: add, switch and sign out",
                a,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| this.open_accounts_sheet(None, cx))),
        )
        .child(inbox_button(unread, a, cx))
        .child(
            footer_button(
                "vault",
                Icon::Key,
                "Vault",
                "Secret Vault: passwords and tokens agents can use without seeing them",
                a,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| this.open_vault_sheet(cx))),
        )
        .child(parts)
        .children(note.map(|note| {
            label(note, TypeRole::Meta, Ink::TextFaint, a).part_at(
                "Daemon Note",
                Corner::TopLeading,
                a,
                cx,
            )
        }))
}

const TOGGLE_LEFT: f32 = 76.0;
const TOGGLE_TOP: f32 = (HEADER_ROW - HEADER_BUTTON) / 2.0;
const TOGGLE_HEIGHT: f32 = HEADER_BUTTON;
const TOGGLE_WIDTH: f32 = HEADER_BUTTON;

pub const HEADER_ROW: f32 = 40.0;
pub const HEADER_BUTTON: f32 = 28.0;
pub const HIDDEN_HEADER_START: f32 = TOGGLE_LEFT + TOGGLE_WIDTH + 4.0 + TOGGLE_WIDTH + 12.0;

pub fn toggle_button(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let hidden = view.sidebar_hidden;
    let tip = if hidden {
        "Show Sidebar"
    } else {
        "Hide Sidebar"
    };
    let button = icon_button(
        "sidebar-toggle",
        Icon::PanelLeft,
        IconSize::Button.points(),
        ButtonKind::Ghost,
        PartState::Rest,
        a,
    )
    .h(px(TOGGLE_HEIGHT))
    .w(px(TOGGLE_WIDTH))
    .px(px(0.0))
    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)));
    div()
        .absolute()
        .top(px(TOGGLE_TOP))
        .left(px(TOGGLE_LEFT))
        .child(gate::tip_text(button, tip.into(), a).part_at(
            "Sidebar Button",
            Corner::Bottom,
            a,
            cx,
        ))
}

pub fn new_session_button(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let left = if view.sidebar_hidden {
        TOGGLE_LEFT + TOGGLE_WIDTH + 4.0
    } else {
        view.sidebar_width() - 12.0 - TOGGLE_WIDTH
    };
    let button = icon_button(
        "new-session",
        Icon::Plus,
        IconSize::Button.points(),
        ButtonKind::Ghost,
        PartState::Rest,
        a,
    )
    .h(px(TOGGLE_HEIGHT))
    .w(px(TOGGLE_WIDTH))
    .px(px(0.0))
    .on_click(cx.listener(|this, _, _, cx| this.open_new_session(cx)));
    div().absolute().top(px(TOGGLE_TOP)).left(px(left)).child(
        gate::tip_text(button, "New Session".into(), a).part_at(
            "New Session Button",
            Corner::Bottom,
            a,
            cx,
        ),
    )
}

fn mark_of(tool: Tool) -> ToolMark {
    match tool {
        Tool::Claude => ToolMark::Claude,
        Tool::Codex => ToolMark::Codex,
        Tool::Antigravity => ToolMark::Antigravity,
    }
}

pub(super) fn model(view: &ChatView, _: &App) -> SidebarModel {
    SidebarModel {
        unread: view.inbox.unread(),
        ..sidebar_model(
            &view.sessions,
            view.now_ms,
            view.selected.as_deref(),
            &view.accounts.snapshot(),
            view.daemon_note,
        )
    }
}

pub(super) fn render(m: &SidebarModel, a: Appearance, cx: &mut Context<ChatView>) -> AnyElement {
    let mut list = div()
        .id("sessions")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .pl(px(8.0))
        .pr(px(GUTTER))
        .flex()
        .flex_col()
        .gap(px(4.0));
    if m.sections.is_empty() {
        list = list.child(div().px(px(8.0)).child(label(
            "No sessions in the last day",
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        )));
    }
    let mut accounts = m.accounts.iter();
    for section in &m.sections {
        list = list.child(
            div()
                .px(px(8.0))
                .pt(px(12.0))
                .pb(px(4.0))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    label(section.title, TypeRole::Section, Ink::TextFaint, a).part_at(
                        "Group Heading",
                        Corner::Trailing,
                        a,
                        cx,
                    ),
                )
                .child(
                    label(
                        &section.rows.len().to_string(),
                        TypeRole::Meta,
                        Ink::TextFaint,
                        a,
                    )
                    .part_at("Group Count", Corner::Bottom, a, cx),
                ),
        );
        for row in &section.rows {
            let id = row.id.clone();
            let state = if m.selected.as_deref() == Some(row.id.as_str()) {
                PartState::Selected
            } else {
                PartState::Rest
            };
            let tool = gate::tip_text(
                tool_icon(mark_of(row.tool), TOOL_ICON, a)
                    .id(SharedString::from(format!("tool-{}", row.id))),
                mark_of(row.tool).name().into(),
                a,
            )
            .part_at("Tool Icon", Corner::Leading, a, cx);
            let account = accounts.next().and_then(Option::as_ref).map(|acc| {
                account_line(&acc.text, acc.lane, a).part_at(
                    "Session Account",
                    Corner::Trailing,
                    a,
                    cx,
                )
            });
            let menu_id = row.id.clone();
            let drop_id = row.id.clone();
            let over = hsla(Ink::Selection.color(a));
            let row_el = session_row_marked(
                SharedString::from(format!("session-{}", row.id)),
                &row.name,
                tool,
                account,
                &row.status_line,
                &row.age,
                row.status,
                state,
                false,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx)))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(over))
            .on_drop::<ExternalPaths>(cx.listener(
                move |this, paths: &ExternalPaths, w, cx| {
                    this.select(&drop_id, cx);
                    this.drop_paths(paths.paths(), w, cx);
                    cx.stop_propagation();
                },
            ));
            list = list.child(
                super::row_menu::on_right_click(row_el, menu_id, cx).part_at(
                    "Session Row",
                    Corner::Trailing,
                    a,
                    cx,
                ),
            );
        }
    }
    div()
        .size_full()
        .fill(Ink::Sidebar, a)
        .border_r(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .flex()
        .flex_col()
        .child(div().h(px(TITLE_BAR)).flex_shrink_0())
        .child(scrollable(list))
        .child(footer(a, m.daemon_note, m.unread, cx))
        .part_inside("Sidebar", Corner::Trailing, a, cx)
        .into_any_element()
}
