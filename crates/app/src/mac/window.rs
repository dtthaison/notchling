use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    div, point, prelude::*, px, size, App, Bounds, Context, DisplayId, Render, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use notchling_kit::metric::{SIDEBAR_WIDTH, SIDE_PADDING, TITLE_BAR, WINDOW_DEFAULT, WINDOW_MIN};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, claude_mark, hsla, label, parts_button, resize_edges, toggle_parts, window_appearance,
    KitStyled, TopBorder,
};
use notchling_kit::{Appearance, Ink, TypeRole};

use crate::window_frame::{place, save, Frame, Screen};

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
    let displays = cx.displays();
    let screens: Vec<Screen> = displays
        .iter()
        .map(|d| Screen {
            id: d.uuid().map(|u| u.to_string()).unwrap_or_default(),
            size: (
                f32::from(d.bounds().size.width),
                f32::from(d.bounds().size.height),
            ),
        })
        .collect();
    let main = cx
        .primary_display()
        .and_then(|p| displays.iter().position(|d| d.id() == p.id()))
        .unwrap_or(0);
    let Some(placed) = place(saved.as_ref(), &screens, main, WINDOW_DEFAULT, WINDOW_MIN) else {
        let bounds = Bounds::centered(None, size(px(WINDOW_DEFAULT.0), px(WINDOW_DEFAULT.1)), cx);
        return (None, WindowBounds::Windowed(bounds));
    };
    let (x, y, w, h) = placed.rect;
    let bounds = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    let bounds = if placed.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };
    (Some(displays[placed.screen].id()), bounds)
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
    if let Err(e) = save(settings, frame) {
        eprintln!("save the window's frame: {e}");
    }
}

pub fn open_main_window<V: Render>(
    cx: &mut App,
    settings: Option<PathBuf>,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> Option<WindowHandle<V>> {
    let saved = settings.as_deref().and_then(crate::window_frame::load);
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
    let pending: Rc<RefCell<Option<Frame>>> = Rc::default();
    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| {
                if let Some(path) = settings.clone() {
                    watch_frame(window, cx, path, pending.clone());
                }
                build(window, cx)
            })
        })
        .map_err(|e| eprintln!("open window: {e}"))
        .ok()?;
    if let Some(path) = settings {
        let on_quit = (path, pending);
        cx.on_app_quit(move |_| {
            flush(&on_quit.0, &on_quit.1);
            async {}
        })
        .detach();
    }
    Some(handle)
}

fn flush(path: &Path, pending: &RefCell<Option<Frame>>) {
    let frame = pending.borrow_mut().take();
    if let Some(frame) = frame {
        save_frame(path, &frame);
    }
}

fn watch_frame<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    path: PathBuf,
    pending: Rc<RefCell<Option<Frame>>>,
) {
    let moves = Rc::new(Cell::new(0u64));
    let (observed_path, observed) = (path.clone(), pending.clone());
    cx.observe_window_bounds(window, move |_, window, cx| {
        *observed.borrow_mut() = Some(frame_of(window, cx));
        let n = moves.get() + 1;
        moves.set(n);
        let (moves, path, pending) = (moves.clone(), observed_path.clone(), observed.clone());
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            if moves.get() == n {
                let frame = pending.borrow_mut().take();
                if let Some(frame) = frame {
                    cx.background_executor()
                        .spawn(async move { save_frame(&path, &frame) })
                        .await;
                }
            }
        })
        .detach();
    })
    .detach();
    window.on_window_should_close(cx, move |_, _| {
        flush(&path, &pending);
        true
    });
}
