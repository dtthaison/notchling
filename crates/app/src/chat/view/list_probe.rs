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

pub const NAME: &str = "chat-list-probe";
pub const ROWS: usize = 300;
pub const LAST: usize = ROWS - 1;
pub const OVERDRAW: f32 = 600.0;
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
pub const WORDS_PER_LINE: usize = 12;

#[derive(Default)]
pub struct Frame {
    pub paints: Cell<u64>,
    pub calls: Cell<u64>,
    pub rows: RefCell<HashMap<usize, Bounds<Pixels>>>,
    pub viewport: Cell<Bounds<Pixels>>,
}

pub fn words_for(ix: usize) -> usize {
    WORDS_PER_LINE * (1 + (ix * 7 + ix / 3) % 12) - 3
}

fn paragraph(ix: usize, words: usize) -> String {
    let body: Vec<&str> = (0..words).map(|k| WORDS[(ix + k) % WORDS.len()]).collect();
    format!("{ix}. {}", body.join(" "))
}

pub struct ProbeList {
    pub state: ListState,
    pub words: Vec<usize>,
    pub frame: Rc<Frame>,
    pub spins: SpinSlots,
    pub infer: bool,
    pub autoscroll: Rc<Cell<bool>>,
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

pub fn build_render(name: &str, cx: &mut App) -> Option<AnyView> {
    (name == NAME).then(|| cx.new(ListProbe::new).into())
}
