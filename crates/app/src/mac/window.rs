//! The main window: how it opens (size, hidden title bar, traffic lights over the sidebar's top) and
//! the empty shell view shown where no data root is known. Chat's own view is `crate::chat::view`.

use gpui::{
    div, point, prelude::*, px, size, App, Bounds, Context, Render, Subscription, TitlebarOptions,
    Window, WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use notchling_kit::metric::{SIDEBAR_WIDTH, SIDE_PADDING, TITLE_BAR, WINDOW_DEFAULT, WINDOW_MIN};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, claude_mark, hsla, label, parts_button, resize_edges, toggle_parts, window_appearance,
    KitStyled, TopBorder,
};
use notchling_kit::{Appearance, Ink, TypeRole};

/// The empty shell: the kit's surfaces in their places, no data.
pub struct ShellView {
    /// Forced by `--render shell-*`; `None` follows the system.
    appearance: Option<Appearance>,
    _appearance_changes: Subscription,
}

impl ShellView {
    pub fn new(
        appearance: Option<Appearance>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = cx.weak_entity();
        let changes = window.observe_window_appearance(move |_, cx| {
            this.update(cx, |_, cx| cx.notify()).ok();
        });
        Self {
            appearance,
            _appearance_changes: changes,
        }
    }
}

/// The shell's sidebar: the surfaces in their places, with the ? button live so the part names
/// still toggle here (there is nothing named in the shell itself).
fn sidebar(a: Appearance, cx: &App) -> impl IntoElement {
    div()
        .w(px(SIDEBAR_WIDTH))
        .h_full()
        .flex_shrink_0()
        .fill(Ink::Sidebar, a)
        .border_r(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .flex()
        .flex_col()
        // The traffic lights sit here; nothing else does.
        .child(div().h(px(TITLE_BAR)).flex_shrink_0())
        .child(div().px(px(SIDE_PADDING)).pb(px(8.0)).child(label(
            "Sessions",
            TypeRole::Section,
            Ink::TextFaint,
            a,
        )))
        .child(div().px(px(SIDE_PADDING)).child(label(
            "No sessions yet",
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        )))
        .child(div().flex_1())
        .child(
            div()
                .h(px(44.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(button(
                    "settings",
                    "Settings",
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                ))
                .child(parts_button("parts", a, cx).on_click(|_, _, cx| {
                    toggle_parts(cx);
                })),
        )
}

impl Render for ShellView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = self.appearance.unwrap_or_else(|| window_appearance(window));
        div()
            .size_full()
            .flex()
            .fill(Ink::Window, a)
            .child(sidebar(a, cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.0))
                    .child(claude_mark(48.0, a))
                    .child(label(
                        "No home folder to read sessions from",
                        TypeRole::Heading,
                        Ink::Text,
                        a,
                    ))
                    .child(label(
                        "Chat lists Claude Code and Codex sessions from ~/.claude and ~/.codex",
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    )),
            )
            .child(resize_edges(window, TopBorder::InContent))
    }
}

/// Opens the main window with `build`'s root view: 1040 × 720, at least 720 × 460, title bar
/// hidden with the traffic lights over the sidebar's top. `None` when GPUI can't open a window.
pub fn open_main_window<V: Render>(
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> Option<WindowHandle<V>> {
    let bounds = Bounds::centered(None, size(px(WINDOW_DEFAULT.0), px(WINDOW_DEFAULT.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Notchling".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.0), px(12.0))),
        }),
        window_min_size: Some(size(px(WINDOW_MIN.0), px(WINDOW_MIN.1))),
        kind: WindowKind::Normal,
        ..Default::default()
    };
    cx.open_window(options, |window, cx| cx.new(|cx| build(window, cx)))
        .map_err(|e| eprintln!("open window: {e}"))
        .ok()
}
