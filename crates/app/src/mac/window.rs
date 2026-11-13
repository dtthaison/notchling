use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    div, point, prelude::*, px, size, App, Bounds, Context, DisplayId, Render, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use notchling_engine::settings::Settings;
use notchling_kit::metric::{SIDEBAR_WIDTH, SIDE_PADDING, TITLE_BAR, WINDOW_DEFAULT, WINDOW_MIN};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, claude_mark, hsla, label, parts_button, resize_edges, toggle_parts, window_appearance,
    KitStyled, TopBorder,
};
use notchling_kit::{Appearance, Ink, TypeRole};

use crate::window_frame::{Frame, KEY};

const SAVE_AFTER: Duration = Duration::from_millis(400);

pub struct ShellView {
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

fn placement(saved: Option<Frame>, cx: &App) -> (Option<DisplayId>, WindowBounds) {
    let default = || {
        let bounds = Bounds::centered(None, size(px(WINDOW_DEFAULT.0), px(WINDOW_DEFAULT.1)), cx);
        (None, WindowBounds::Windowed(bounds))
    };
    let Some(saved) = saved else {
        return default();
    };
    let Some(display) = cx
        .displays()
        .into_iter()
        .find(|d| d.uuid().is_ok_and(|u| u.to_string() == saved.display))
    else {
        return default();
    };
    let screen = display.bounds().size;
    let (x, y, w, h) = saved.fit(
        (f32::from(screen.width), f32::from(screen.height)),
        WINDOW_MIN,
    );
    let bounds = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    let bounds = if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };
    (Some(display.id()), bounds)
}

fn frame_of(window: &Window, cx: &App) -> Frame {
    let display = window
        .display(cx)
        .and_then(|d| d.uuid().ok())
        .map(|u| u.to_string())
        .unwrap_or_default();
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(b) | WindowBounds::Fullscreen(b) => (b, false),
        WindowBounds::Maximized(b) => (b, true),
    };
    Frame {
        display,
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        maximized,
    }
}

fn save_frame(settings: &Path, frame: &Frame) {
    match Settings::open(settings) {
        Ok(mut store) => {
            if let Err(e) = store.set(KEY, frame.to_value()) {
                eprintln!("save the window's frame: {e}");
            }
        }
        Err(e) => eprintln!("save the window's frame: {e}"),
    }
}

pub fn open_main_window<V: Render>(
    cx: &mut App,
    settings: Option<PathBuf>,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> Option<WindowHandle<V>> {
    let saved = settings
        .as_deref()
        .and_then(|p| Settings::open(p).ok())
        .and_then(|s| Frame::from_value(s.get(KEY).as_ref()));
    let (display_id, bounds) = placement(saved, cx);
    let options = WindowOptions {
        window_bounds: Some(bounds),
        display_id,
        titlebar: Some(TitlebarOptions {
            title: Some("Notchling".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.0), px(12.0))),
        }),
        window_min_size: Some(size(px(WINDOW_MIN.0), px(WINDOW_MIN.1))),
        kind: WindowKind::Normal,
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        cx.new(|cx| {
            if let Some(path) = settings {
                let moves = Rc::new(Cell::new(0u64));
                cx.observe_window_bounds(window, move |_, window, cx| {
                    let frame = frame_of(window, cx);
                    let n = moves.get() + 1;
                    moves.set(n);
                    let (moves, path) = (moves.clone(), path.clone());
                    cx.spawn(async move |_, cx| {
                        cx.background_executor().timer(SAVE_AFTER).await;
                        if moves.get() == n {
                            save_frame(&path, &frame);
                        }
                    })
                    .detach();
                })
                .detach();
            }
            build(window, cx)
        })
    })
    .map_err(|e| eprintln!("open window: {e}"))
    .ok()
}
