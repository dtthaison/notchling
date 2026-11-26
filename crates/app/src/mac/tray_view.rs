use gpui::{
    canvas, div, point, prelude::*, px, rgb, AnyView, App, Bounds, Context, Hsla, PathBuilder,
    Pixels, Render, Window,
};
use notchling_kit::ui::{
    hsla, label, menu_panel, menu_rule, window_appearance, KitStyled, MenuItem,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use crate::menu::{tray_look, tray_menu, Dots, Entry, MenuState, TrayGlyph, TrayLook};
use crate::tray_glyph::{
    circle_polygon, frames_of, marks_of, pill_polygon, Frame, Mark, Tint, HEIGHT, PILLS, STROKE,
    WIDTH,
};
use crate::tray_render::{blocked_state, idle_state};

const MENU_WIDTH: f32 = 300.0;
const SUBMENU_TOP: f32 = 30.0;

fn tint_color(tint: Tint, a: Appearance) -> Hsla {
    let dark = a == Appearance::Dark;
    let value = match (tint, dark) {
        (Tint::Green, true) => 0x30D158,
        (Tint::Green, false) => 0x28CD41,
        (Tint::Orange, true) => 0xFF9F0A,
        (Tint::Orange, false) => 0xFF9500,
        (Tint::Red, true) => 0xFF453A,
        (Tint::Red, false) => 0xFF3B30,
    };
    rgb(value).into()
}

fn at(origin: gpui::Point<Pixels>, scale: f32, (x, y): (f64, f64)) -> gpui::Point<Pixels> {
    point(
        origin.x + px(x as f32 * scale),
        origin.y + px(y as f32 * scale),
    )
}

fn paint_frame(
    bounds: Bounds<Pixels>,
    frame: Option<Frame>,
    filled: bool,
    scale: f32,
    a: Appearance,
    window: &mut Window,
) {
    let ink = hsla(Ink::Text.color(a));
    for index in 0..PILLS.len() {
        let points: Vec<_> = pill_polygon(index, 10)
            .into_iter()
            .map(|p| at(bounds.origin, scale, p))
            .collect();
        if filled {
            let mut builder = PathBuilder::fill();
            builder.add_polygon(&points, true);
            if let Ok(path) = builder.build() {
                window.paint_path(path, ink);
            }
        }
        let mut builder = PathBuilder::stroke(px(STROKE as f32 * scale));
        builder.add_polygon(&points, true);
        if let Ok(path) = builder.build() {
            window.paint_path(path, ink);
        }
    }
    let (marks, color) = match frame {
        Some(frame) => (
            frame.marks,
            tint_color(frame.tint, a).opacity(frame.alpha as f32),
        ),
        None => (marks_of(Dots::Idle), ink),
    };
    for mark in marks {
        let (center, radius, line) = match mark {
            Mark::Bead { x, y, radius } => ((x, y), radius, None),
            Mark::Ring { x, y, radius, line } => ((x, y), radius, Some(line)),
        };
        let points: Vec<_> = circle_polygon(center.0, center.1, radius, 28)
            .into_iter()
            .map(|p| at(bounds.origin, scale, p))
            .collect();
        let mut builder = match line {
            Some(line) => PathBuilder::stroke(px(line as f32 * scale)),
            None => PathBuilder::fill(),
        };
        builder.add_polygon(&points, true);
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}

fn glyph(look: TrayLook, frame_index: usize, scale: f32, a: Appearance) -> impl IntoElement {
    let frames = frames_of(&look);
    let frame = frames.get(frame_index).cloned();
    let filled = look.glyph == TrayGlyph::Filled;
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| paint_frame(bounds, frame, filled, scale, a, window),
    )
    .w(px(WIDTH as f32 * scale))
    .h(px(HEIGHT as f32 * scale))
}

fn state_of(dots: Dots) -> MenuState {
    MenuState {
        working: dots != Dots::Idle,
        waiting_count: usize::from(dots == Dots::Waiting),
        blocking: if dots == Dots::Blocked {
            blocked_state().blocking
        } else {
            Vec::new()
        },
        ..MenuState::default()
    }
}

fn key_text(key: Option<char>) -> Option<String> {
    key.map(|k| format!("⌘{}", k.to_ascii_uppercase()))
}

fn item(index: &str, entry: &Entry, a: Appearance) -> gpui::AnyElement {
    match entry {
        Entry::Separator => menu_rule(a).into_any_element(),
        Entry::Header(title) => MenuItem::new(format!("h-{index}"), title.clone())
            .held(true)
            .build(a)
            .into_any_element(),
        Entry::Disabled(title) => MenuItem::new(format!("d-{index}"), title.clone())
            .held(true)
            .build(a)
            .into_any_element(),
        Entry::Submenu { title, .. } => MenuItem::new(format!("s-{index}"), title.clone())
            .chevron(Icon::ChevronRight)
            .build(a)
            .into_any_element(),
        Entry::Action {
            title,
            key,
            checked,
            ..
        } => {
            let mut built = MenuItem::new(format!("a-{index}"), title.clone());
            if let Some(text) = key_text(*key) {
                built = built.shortcut(text);
            }
            if *checked {
                built = built.checked(true);
            }
            built.build(a).into_any_element()
        }
    }
}

fn panel(entries: &[Entry], a: Appearance) -> gpui::Div {
    entries
        .iter()
        .enumerate()
        .fold(menu_panel(a).w(px(MENU_WIDTH)), |panel, (i, entry)| {
            panel.child(item(&i.to_string(), entry, a))
        })
}

struct MenuRender {
    state: MenuState,
    open_first: bool,
    forced: Option<Appearance>,
}

impl Render for MenuRender {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let a = self.forced.unwrap_or_else(|| window_appearance(window));
        let look = tray_look(&self.state, false);
        let entries = tray_menu(&self.state);
        let sub = self.open_first.then(|| {
            entries.iter().find_map(|e| match e {
                Entry::Submenu { items, .. } => Some(items.clone()),
                _ => None,
            })
        });
        let mut row = div()
            .flex()
            .gap(px(4.0))
            .items_start()
            .child(panel(&entries, a));
        if let Some(Some(items)) = sub {
            row = row.child(
                div()
                    .mt(px(SUBMENU_TOP))
                    .child(panel(&items, a).w(px(180.0))),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .fill(Ink::Window, a)
            .child(
                div()
                    .h(px(26.0))
                    .w_full()
                    .px(px(14.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(14.0))
                    .fill(Ink::Sidebar, a)
                    .child(glyph(look, 0, 1.0, a))
                    .child(label("21:14", TypeRole::Meta, Ink::Text, a)),
            )
            .child(div().p(px(14.0)).child(row))
    }
}

struct GlyphRender {
    forced: Option<Appearance>,
}

const STATES: [(Dots, &str); 5] = [
    (Dots::Idle, "Idle"),
    (Dots::Working, "Working"),
    (Dots::Waiting, "Waits on you"),
    (Dots::Blocked, "Blocked"),
    (Dots::Blocked, "Blocked, pulse low"),
];

fn strip(a: Appearance, scale: f32, filled: bool) -> gpui::Div {
    let mut bar = div()
        .flex()
        .gap(px(18.0))
        .items_center()
        .p(px(10.0))
        .fill(Ink::Window, a);
    for (index, (dots, name)) in STATES.iter().enumerate() {
        let mut state = state_of(*dots);
        state.holding = filled;
        let look = tray_look(&state, false);
        let frame = if index == 4 { 4 } else { 0 };
        bar = bar.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.0))
                .child(glyph(look, frame, scale, a))
                .child(label(name, TypeRole::Meta, Ink::TextMuted, a)),
        );
    }
    bar
}

impl Render for GlyphRender {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let a = self.forced.unwrap_or_else(|| window_appearance(window));
        let (first, second) = (
            a,
            if a == Appearance::Dark {
                Appearance::Light
            } else {
                Appearance::Dark
            },
        );
        div()
            .size_full()
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .fill(Ink::Window, a)
            .child(label(
                "At size (22 x 16)",
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(strip(first, 1.0, false))
            .child(strip(second, 1.0, false))
            .child(label(
                "Zoomed 3x, outline and filled",
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(strip(first, 3.0, false))
            .child(strip(first, 3.0, true))
    }
}

pub fn build_render(name: &str, forced: Option<Appearance>, cx: &mut App) -> Option<AnyView> {
    match name {
        "tray-menu" => Some(
            cx.new(|_| MenuRender {
                state: blocked_state(),
                open_first: true,
                forced,
            })
            .into(),
        ),
        "tray-menu-idle" => Some(
            cx.new(|_| MenuRender {
                state: idle_state(),
                open_first: false,
                forced,
            })
            .into(),
        ),
        "tray-glyphs" => Some(cx.new(|_| GlyphRender { forced }).into()),
        _ => None,
    }
}
