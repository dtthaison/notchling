use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, font, point, prelude::*, px, quad, size, App, BorderStyle, Bounds, Div, EntityId,
    FontWeight, Global, Pixels, Styled, TextRun, Window,
};
use notchling_kit::labels::{BADGE_HEIGHT, BADGE_PADDING, BADGE_TEXT, OUTLINE_RADIUS};
use notchling_kit::render_map::{badge_text, Reading, Region, Track};
use notchling_kit::ui::{hsla, parts_on as labels_on, RegionScope, SYSTEM_FONT};
use notchling_kit::{Appearance, Ink};

use super::pane;

pub const MAP_ENV: &str = "NOTCHLING_RENDER_MAP";

pub fn map_on(cx: &App) -> bool {
    static OFF: OnceLock<bool> = OnceLock::new();
    labels_on(cx) && !*OFF.get_or_init(|| std::env::var_os(MAP_ENV).is_some_and(|v| v == "0"))
}

pub fn scope(region: Region, cx: &App) -> RegionScope {
    RegionScope::enter(map_on(cx).then_some(region))
}

const STROKE: f32 = 2.0;
const INSET: f32 = 2.0;
const OUTLINE_ALPHA: f32 = 0.9;
const EDGE: f32 = 4.0;

#[derive(Clone, Copy)]
pub enum Place {
    Top(f32),
    Bottom(f32),
}

struct MapState {
    epoch: Instant,
    tracks: [Track; 4],
    timer: bool,
    chrome: Option<EntityId>,
}

impl Global for MapState {}

impl Default for MapState {
    fn default() -> Self {
        MapState {
            epoch: Instant::now(),
            tracks: Default::default(),
            timer: false,
            chrome: None,
        }
    }
}

impl MapState {
    fn track(&mut self, region: Region) -> &mut Track {
        let i = Region::ALL.iter().position(|r| *r == region).unwrap_or(0);
        &mut self.tracks[i]
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}

pub fn sync(chrome: EntityId, cx: &mut App) {
    if map_on(cx) {
        cx.default_global::<MapState>().chrome = Some(chrome);
    } else if cx.has_global::<MapState>() {
        cx.remove_global::<MapState>();
    }
}

fn counters(region: Region) -> (u64, u64) {
    let load = |c: &std::sync::atomic::AtomicU64| c.load(Ordering::Relaxed);
    match region {
        Region::Sidebar => (load(&pane::SIDEBAR_RENDERS), 0),
        Region::Conversation => (load(&pane::RENDERS), load(&pane::ROWS)),
        Region::Details => (load(&pane::DETAILS_RENDERS), 0),
        Region::Window => (load(&pane::ROOT_RENDERS), 0),
    }
}

fn outline(region: Region, a: Appearance) -> Div {
    div()
        .absolute()
        .top(px(INSET))
        .left(px(INSET))
        .right(px(INSET))
        .bottom(px(INSET))
        .rounded(px(OUTLINE_RADIUS))
        .border(px(STROKE))
        .border_color(hsla(region.ink().color(a).with_alpha(OUTLINE_ALPHA)))
}

fn badge(region: Region, place: Place, a: Appearance) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| paint_badge(region, place, a, bounds, window, cx),
    )
    .absolute()
    .top_0()
    .left_0()
    .right_0()
    .bottom_0()
}

pub fn overlay(region: Region, place: Place, a: Appearance, cx: &App) -> Option<Div> {
    map_on(cx).then(|| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .child(outline(region, a))
            .child(badge(region, place, a))
    })
}

pub trait WindowRegion: Styled + ParentElement + Sized {
    fn window_region(mut self, a: Appearance, cx: &App) -> Self {
        if !map_on(cx) {
            return self;
        }
        if self.style().position.is_none() {
            self.style().position = Some(gpui::Position::Relative);
        }
        self.child(outline(Region::Window, a))
    }

    fn window_region_badge(self, a: Appearance, cx: &App) -> Self {
        let region = self.window_region(a, cx);
        if !map_on(cx) {
            return region;
        }
        region.child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .child(badge(Region::Window, Place::Top(INSET + STROKE + 1.0), a)),
        )
    }
}

impl<T: Styled + ParentElement> WindowRegion for T {}

fn paint_badge(
    region: Region,
    place: Place,
    a: Appearance,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let (draws, rows) = counters(region);
    let (reading, wake) = read(region, draws, rows, cx);
    let text = badge_text(
        region,
        reading.draws,
        (region == Region::Conversation).then_some(reading.rows),
    );
    let (fill_ink, text_ink) = if reading.flashing {
        (region.ink(), region.on_ink())
    } else {
        (Ink::Card, Ink::Text)
    };
    let mut f = font(SYSTEM_FONT);
    f.weight = FontWeight::SEMIBOLD;
    let run = TextRun {
        len: text.len(),
        font: f,
        color: hsla(text_ink.color(a)),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.into(), px(BADGE_TEXT), &[run], None);
    let width = line.width + px(BADGE_PADDING * 2.0);
    let height = px(BADGE_HEIGHT);
    let centred = bounds.origin.x + (bounds.size.width - width) / 2.0;
    let x = centred.max(bounds.origin.x + px(EDGE));
    let y = match place {
        Place::Top(from) => bounds.origin.y + px(from),
        Place::Bottom(from) => bounds.origin.y + bounds.size.height - px(from) - height,
    };
    let pill = Bounds::new(point(x, y), size(width, height));
    window.paint_quad(quad(
        pill,
        height / 2.0,
        hsla(fill_ink.color(a)),
        px(1.0),
        hsla(region.ink().color(a)),
        BorderStyle::Solid,
    ));
    line.paint(point(x + px(BADGE_PADDING), y), height, window, cx)
        .ok();
    if let Some(ms) = wake {
        wake_once(ms, cx);
    }
}

fn read(region: Region, draws: u64, rows: u64, cx: &mut App) -> (Reading, Option<u64>) {
    let state = cx.default_global::<MapState>();
    let now = state.now_ms();
    let track = state.track(region);
    let reading = track.observe(draws, rows, now);
    let wake = track.wake_in(now);
    if wake.is_some() && !state.timer {
        state.timer = true;
        (reading, wake)
    } else {
        (reading, None)
    }
}

fn wake_once(ms: u64, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(ms))
            .await;
        cx.update(|cx| {
            if !cx.has_global::<MapState>() {
                return;
            }
            let state = cx.default_global::<MapState>();
            state.timer = false;
            state.track(Region::Window).ignore_next_draw();
            if let Some(id) = state.chrome {
                cx.notify(id);
            }
        })
        .ok();
    })
    .detach();
}
