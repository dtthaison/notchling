//! A render-only probe of GPUI's `list` (`--render chat-list-probe`, Task 2.1 of the Chat state
//! split), kept as a regression probe. It builds what the conversation would need: a cached child
//! view (as `ConversationView` will be) holding a bottom-aligned `ListState` over 300 made-up
//! rows of one to twelve wrapped lines, the last with a kit spinner, drawn through a spinner host
//! and a `SpinLayer` like the window's parts. With `NOTCHLING_RENDER_BENCH=<seconds>` set it runs
//! its checks one frame at a time ([`super::list_probe_run`]), prints `list-probe: {...}` and
//! exits; without it, it only shows the list.
//!
//! What the rows record while they paint is the truth the checks read: each row's painted bounds
//! and the viewport's, so a check never trusts the list's own bookkeeping alone. The list is drawn
//! through the kit's scroll bar (`scrollable_list`), whose thumb gate G7 reads.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    canvas, div, list, prelude::*, px, size, AnyElement, AnyView, App, Bounds, Context, Entity,
    ListAlignment, ListSizingBehavior, ListState, Pixels, StyleRefinement, Window,
};
use notchling_kit::ui::{
    drop_spinner_host, host_spinners, scrollable_list, spin_host, spinner, window_appearance,
    KitStyled, SpinLayer, SpinSlots, ThumbRecord,
};
use notchling_kit::{Appearance, Ink, TypeRole};

/// The view's name for `--render`.
pub const NAME: &str = "chat-list-probe";
/// Rows in the list, and the last one's index.
pub const ROWS: usize = 300;
pub const LAST: usize = ROWS - 1;
/// Space the list measures beyond the viewport, as the conversation would set it.
pub const OVERDRAW: f32 = 600.0;
/// The viewport's height and the list's two widths (before and after a resize).
pub const VIEWPORT: f32 = 600.0;
pub const WIDE: f32 = 600.0;
pub const NARROW: f32 = 480.0;

const WORDS: [&str; 8] = [
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "elit",
    "tempor",
];
/// Words in a row's text that fill one line at the wide width, about.
pub const WORDS_PER_LINE: usize = 12;

/// What the rows record while they paint, shared with the checks.
#[derive(Default)]
pub struct Frame {
    /// Times the child view painted: one per frame it was drawn in.
    pub paints: Cell<u64>,
    /// Times a row was asked for (`render_item`).
    pub calls: Cell<u64>,
    /// The rows painted in the latest frame, by index, in window coordinates.
    pub rows: RefCell<HashMap<usize, Bounds<Pixels>>>,
    /// The viewport as painted.
    pub viewport: Cell<Bounds<Pixels>>,
}

/// The made-up number of words in row `ix`: one to twelve lines at the wide width.
pub fn words_for(ix: usize) -> usize {
    WORDS_PER_LINE * (1 + (ix * 7 + ix / 3) % 12) - 3
}

fn paragraph(ix: usize, words: usize) -> String {
    let body: Vec<&str> = (0..words).map(|k| WORDS[(ix + k) % WORDS.len()]).collect();
    format!("{ix}. {}", body.join(" "))
}

/// The cached child: the list and what it records.
pub struct ProbeList {
    pub state: ListState,
    /// Words in each row's text; a check grows a row by changing its number and splicing it.
    pub words: Vec<usize>,
    pub frame: Rc<Frame>,
    pub spins: SpinSlots,
    /// Lay the rows out in `request_layout` too (the list's default), or only in prepaint.
    pub infer: bool,
    /// The last row asks to be scrolled into view, with room below it, while it paints.
    pub autoscroll: Rc<Cell<bool>>,
    /// Where the kit's scroll bar put its thumb in the latest frame (gate G7).
    pub thumb: ThumbRecord,
    _release: gpui::Subscription,
}

impl ProbeList {
    fn new(cx: &mut Context<Self>) -> ProbeList {
        let spins = SpinSlots::default();
        let id = cx.entity_id();
        host_spinners(cx, id, &spins);
        ProbeList {
            state: ListState::new(ROWS, ListAlignment::Bottom, px(OVERDRAW)),
            words: (0..ROWS).map(words_for).collect(),
            frame: Rc::new(Frame::default()),
            spins,
            infer: false,
            autoscroll: Rc::new(Cell::new(false)),
            thumb: Rc::default(),
            _release: cx.on_release(move |_, cx| drop_spinner_host(cx, id)),
        }
    }

    fn row(&mut self, ix: usize, window: &mut Window) -> AnyElement {
        self.frame.calls.set(self.frame.calls.get() + 1);
        let a = window_appearance(window);
        let frame = self.frame.clone();
        let wants_room = ix == LAST && self.autoscroll.get();
        let text = paragraph(ix, self.words[ix]);
        div()
            .w_full()
            .relative()
            .flex()
            .flex_col()
            .pt(px(14.0))
            .px(px(16.0))
            .child(notchling_kit::ui::label(
                &text,
                TypeRole::Body,
                Ink::Text,
                a,
            ))
            .when(ix == LAST, |d| {
                d.child(spinner("probe-spin", 10.0, Ink::Working, a))
            })
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, _, _| {
                        frame.rows.borrow_mut().insert(ix, bounds);
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .when(wants_room, |d| {
                d.child(
                    canvas(
                        |bounds, window, _| {
                            // Room for 60 pt more below the row: past the viewport's end, so the
                            // list scrolls, which makes it lay its rows out a second time.
                            window.request_autoscroll(Bounds {
                                origin: bounds.origin,
                                size: size(bounds.size.width, bounds.size.height + px(60.0)),
                            });
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
            })
            .into_any_element()
    }
}

impl Render for ProbeList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.spins.clear();
        self.frame.rows.borrow_mut().clear();
        let rows = list(
            self.state.clone(),
            cx.processor(|this, ix: usize, window, _| this.row(ix, window)),
        )
        .size_full()
        .with_sizing_behavior(if self.infer {
            ListSizingBehavior::Infer
        } else {
            ListSizingBehavior::Auto
        });
        let frame = self.frame.clone();
        // Through the kit's bar, as the conversation draws its list (Task 6.2).
        let bar = scrollable_list("probe-list", rows, &self.state).record_thumb(&self.thumb);
        div().size_full().relative().child(bar).child(
            canvas(
                |bounds, _, _| bounds,
                move |bounds, _, _, _| {
                    frame.viewport.set(bounds);
                    frame.paints.set(frame.paints.get() + 1);
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }
}

/// The window's content: the list's column at a width the checks change, and the spinner layer.
pub struct ListProbe {
    pub list: Entity<ProbeList>,
    layer: Entity<SpinLayer>,
    pub width: f32,
}

impl ListProbe {
    fn new(cx: &mut Context<Self>) -> ListProbe {
        let list = cx.new(ProbeList::new);
        let hosts = vec![list.read(cx).spins.clone()];
        let layer = cx.new(|_| SpinLayer::new(hosts));
        if std::env::var_os(crate::mac::render::BENCH_ENV).is_some() {
            super::list_probe_run::start(list.clone(), cx);
        }
        ListProbe {
            list,
            layer,
            width: WIDE,
        }
    }
}

impl Render for ListProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a: Appearance = window_appearance(window);
        let child = AnyView::from(self.list.clone()).cached(StyleRefinement::default().size_full());
        let slots = self.list.read(cx).spins.clone();
        div()
            .size_full()
            .fill(Ink::Window, a)
            .p(px(20.0))
            .child(
                div()
                    .w(px(self.width))
                    .h(px(VIEWPORT))
                    .border_1()
                    .border_color(notchling_kit::ui::hsla(Ink::Hairline.color(a)))
                    .child(spin_host(&slots, child)),
            )
            .child(self.layer.clone())
    }
}

/// `--render chat-list-probe`.
pub fn build_render(name: &str, cx: &mut App) -> Option<AnyView> {
    (name == NAME).then(|| cx.new(ListProbe::new).into())
}
