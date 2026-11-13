use std::sync::Arc;

use gpui::{
    canvas, div, img, point, prelude::*, px, relative, rgb, AnyElement, Context, Div,
    ExternalPaths, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, PathBuilder,
    RenderImage, SharedString, Stateful, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{MENU_RADIUS, MENU_ROW_X, MENU_ROW_Y};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, hsla, icon, icon_button, label, menu_panel, menu_rule, KitPart, KitStyled, MenuItem,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::super::render_map::WindowRegion;
use super::super::ChatView;
use super::{NewSimulator, AREA_PAD, HEADER, PEN, PEN_SHARE, WIDTH};
use crate::chat::page_panel::panel_width;
use crate::chat::simulator::{frame_for, menu, Device, SimKey, Zoom};
use crate::mac::gate;

fn tool(
    id: &'static str,
    shape: Icon,
    tip: &'static str,
    on: bool,
    enabled: bool,
    a: Appearance,
) -> Stateful<Div> {
    let kind = if on {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    let state = if enabled {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    gate::tip_text(
        icon_button(id, shape, IconSize::Button.points(), kind, state, a),
        tip.into(),
        a,
    )
    .flex_shrink_0()
}

fn header(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let s = &view.simulator;
    let device = s.device();
    let name = device
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "Simulator".into());
    let runtime = device.map(|d| d.runtime.clone()).unwrap_or_default();
    let who = div()
        .id("sim-device")
        .flex_shrink()
        .min_w(px(0.0))
        .px(px(8.0))
        .py(px(4.0))
        .rounded(px(8.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .when(s.menu_open, |d| d.fill(Ink::Raised, a))
        .hover(move |st| st.bg(hsla(Ink::Raised.color(a))))
        .child(
            label(&name, TypeRole::Label, Ink::Text, a)
                .elide()
                .min_w(px(0.0)),
        )
        .when(!runtime.is_empty(), |d| {
            d.child(label(&runtime, TypeRole::Meta, Ink::TextMuted, a).flex_shrink_0())
        })
        .child(icon(Icon::ChevronDown, 11.0, Ink::TextMuted, a))
        .on_click(cx.listener(|this, _, _, cx| {
            this.simulator.menu_open = !this.simulator.menu_open;
            cx.notify();
        }));
    let has = s.frame.is_some() && device.is_some() && s.annotation.is_none();
    let fit = matches!(s.zoom(), Zoom::Fit);
    let actual = matches!(s.zoom(), Zoom::Times(t) if (t - 1.0).abs() < 0.001);
    let out = tool("sim-zoom-out", Icon::ZoomOut, "Zoom Out", false, has, a).when(has, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.step_zoom(false, cx)))
    });
    let fit_b = tool(
        "sim-zoom-fit",
        Icon::ZoomFit,
        "Fit to the Panel",
        fit,
        has,
        a,
    )
    .when(has, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.set_zoom(Zoom::Fit, cx)))
    });
    let one = gate::tip_text(
        button(
            "sim-zoom-actual",
            "1×",
            if actual {
                ButtonKind::Secondary
            } else {
                ButtonKind::Ghost
            },
            if has {
                PartState::Rest
            } else {
                PartState::Disabled
            },
            a,
        )
        .px(px(8.0)),
        "Actual Size".into(),
        a,
    )
    .when(has, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.set_zoom(Zoom::Times(1.0), cx)))
    });
    let zin = tool("sim-zoom-in", Icon::ZoomIn, "Zoom In", false, has, a).when(has, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.step_zoom(true, cx)))
    });
    div()
        .h(px(HEADER))
        .flex_shrink_0()
        .pl(px(6.0))
        .pr(px(8.0))
        .flex()
        .items_center()
        .gap(px(2.0))
        .child(who)
        .child(div().flex_1())
        .child(out)
        .child(fit_b)
        .child(one)
        .child(zin)
}

fn device_row(
    id: String,
    d: &Device,
    ticked: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let picked = d.clone();
    MenuItem::new(id, d.name.clone())
        .detail(&d.runtime)
        .checked(ticked)
        .build(a)
        .on_click(cx.listener(move |this, _, _, cx| this.pick_simulator(&picked, cx)))
}

fn menu_action(id: &'static str, title: &str, enabled: bool, a: Appearance) -> Stateful<Div> {
    MenuItem::new(id, title.to_owned()).held(!enabled).build(a)
}

fn section(title: &str, dot: bool, a: Appearance) -> Div {
    div()
        .px(px(MENU_ROW_X))
        .pt(px(MENU_ROW_Y))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(label(title, TypeRole::Meta, Ink::TextMuted, a))
        .when(dot, |d| {
            d.child(div().size(px(6.0)).rounded_full().fill(Ink::Working, a))
        })
}

fn device_menu(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let s = &view.simulator;
    if !s.menu_open {
        return None;
    }
    let (booted, available) = menu(&s.all);
    let shown = s.device().cloned();
    let has = shown.is_some();
    let mut items = menu_panel(a).child(section("Booted", !booted.is_empty(), a));
    if booted.is_empty() {
        items = items.child(menu_action("sim-none", "None", false, a));
    }
    for (i, d) in booted.iter().enumerate() {
        let ticked = shown.as_ref().is_some_and(|s| s.udid == d.udid);
        items = items.child(device_row(format!("sim-booted-{i}"), d, ticked, a, cx));
    }
    if !available.is_empty() {
        items = items
            .child(menu_rule(a))
            .child(section("Available (will boot)", false, a));
        for (i, d) in available.iter().enumerate() {
            items = items.child(device_row(format!("sim-avail-{i}"), d, false, a, cx));
        }
    }
    let shut = shown
        .as_ref()
        .map(|d| format!("Shut Down {}", d.name))
        .unwrap_or_else(|| "Shut Down".into());
    items = items
        .child(menu_rule(a))
        .child(
            menu_action("sim-add", "Add Simulator…", true, a)
                .on_click(cx.listener(|this, _, _, cx| this.begin_new_simulator(cx))),
        )
        .child(menu_rule(a))
        .child(
            menu_action("sim-media", "Add Photos or Videos…", has, a).when(has, |r| {
                r.on_click(cx.listener(|this, _, _, cx| this.pick_media(cx)))
            }),
        )
        .child(
            menu_action("sim-open", "Open in Simulator", true, a)
                .on_click(cx.listener(|this, _, _, cx| this.open_simulator_app(cx))),
        )
        .child(
            menu_action("sim-detach", "Detach Simulator", true, a)
                .on_click(cx.listener(|this, _, _, cx| this.close_simulator(cx))),
        )
        .child(menu_action("sim-shutdown", &shut, has, a).when(has, |r| {
            r.on_click(cx.listener(|this, _, _, cx| this.shut_down_simulator(cx)))
        }));
    Some(
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("sim-menu-away")
                    .absolute()
                    .size_full()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.simulator.menu_open = false;
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(8.0))
                    .top(px(HEADER - 4.0))
                    .occlude()
                    .rounded(px(MENU_RADIUS))
                    .child(items),
            ),
    )
}

fn new_simulator_form(form: &NewSimulator, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let mut runtimes = div().flex().flex_wrap().gap(px(6.0));
    for (i, r) in form.runtimes.iter().enumerate() {
        let on = i == form.runtime;
        runtimes = runtimes.child(
            button(
                SharedString::from(format!("sim-rt-{i}")),
                &r.name,
                if on {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Ghost
                },
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(f) = this.simulator.adding.as_mut() {
                    f.runtime = i;
                    f.kind = 0;
                }
                cx.notify();
            })),
        );
    }
    let types = form
        .runtimes
        .get(form.runtime)
        .map(|r| r.types.clone())
        .unwrap_or_default();
    let mut list = div().flex().flex_col().gap(px(2.0));
    for (i, (name, _)) in types.iter().enumerate() {
        let on = i == form.kind;
        list = list.child(
            div()
                .id(SharedString::from(format!("sim-type-{i}")))
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .cursor_pointer()
                .when(on, |r| r.fill(Ink::Raised, a))
                .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
                .child(label(name, TypeRole::Label, Ink::Text, a).flex_1())
                .when(on, |r| r.child(icon(Icon::Check, 12.0, Ink::Accent, a)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(f) = this.simulator.adding.as_mut() {
                        f.kind = i;
                    }
                    cx.notify();
                })),
        );
    }
    let can = !types.is_empty();
    div()
        .flex_1()
        .min_h(px(0.0))
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(label("New Simulator", TypeRole::Heading, Ink::Text, a))
        .child(runtimes)
        .child(
            div()
                .id("sim-types")
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .child(list),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(
                    button(
                        "sim-new-cancel",
                        "Cancel",
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.simulator.adding = None;
                        cx.notify();
                    })),
                )
                .child(
                    button(
                        "sim-new-create",
                        "Create and Boot",
                        ButtonKind::Primary,
                        if can {
                            PartState::Rest
                        } else {
                            PartState::Disabled
                        },
                        a,
                    )
                    .when(can, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.create_simulator(cx)))
                    }),
                ),
        )
}

fn phone(
    view: &ChatView,
    image: Arc<RenderImage>,
    device: &Device,
    cx: &mut Context<ChatView>,
) -> Div {
    let s = &view.simulator;
    let size = image.size(0);
    let pixels = (size.width.0 as f32, size.height.0 as f32);
    let room = s.room.get();
    let room = if room.0 < 1.0 {
        (WIDTH - 2.0 * AREA_PAD, 700.0)
    } else {
        room
    };
    let guess = frame_for(room.0, device.ipad()).0;
    let (w, h) = s.zoom().size(pixels, device.scale(), room, guess);
    let (bezel, corner) = frame_for(w, device.ipad());
    let portrait = h >= w;
    let strokes: Vec<Vec<(f32, f32)>> = s
        .annotation
        .as_ref()
        .map(|n| n.strokes.clone())
        .unwrap_or_default();
    let annotating = s.annotation.is_some();
    let screen_at = s.screen_at.clone();
    let pen_width = (w * PEN_SHARE).max(1.5);
    let overlay = canvas(
        move |bounds, _, _| {
            screen_at.set(bounds);
            bounds
        },
        move |_, bounds, window, _| {
            let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let at = |p: (f32, f32)| point(px(ox + p.0 * bw), px(oy + p.1 * bh));
            for stroke in &strokes {
                let Some(first) = stroke.first().copied() else {
                    continue;
                };
                let mut path = PathBuilder::stroke(px(pen_width));
                path.move_to(at(first));
                if stroke.len() == 1 {
                    path.line_to(at((first.0 + 0.002, first.1)));
                }
                for p in &stroke[1..] {
                    path.line_to(at(*p));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(PEN));
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let mut screen = div()
        .id("sim-screen")
        .relative()
        .w(px(w))
        .h(px(h))
        .rounded(px(corner))
        .overflow_hidden()
        .bg(rgb(0x000000))
        .child(
            img(image)
                .size_full()
                .rounded(px(corner))
                .object_fit(ObjectFit::Fill),
        )
        .child(overlay);
    if annotating {
        screen = screen
            .cursor_crosshair()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| this.pen_down(e.position, cx)),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if e.pressed_button == Some(MouseButton::Left) {
                    this.pen_move(e.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.pen_up()),
            );
    }
    let tall = h + 2.0 * bezel;
    let side = |left: bool, top: f32, length: f32| {
        let b = div()
            .absolute()
            .top(px(tall * top))
            .w(px(3.0))
            .h(px(tall * length))
            .rounded(px(1.5))
            .bg(rgb(0x3a3a3c));
        if left {
            b.left(px(-4.5))
        } else {
            b.right(px(-4.5))
        }
    };
    let buttons = (portrait && !device.ipad()).then(|| {
        vec![
            side(true, 0.17, 0.035),
            side(true, 0.24, 0.065),
            side(true, 0.32, 0.065),
            side(false, 0.27, 0.1),
        ]
    });
    div()
        .relative()
        .flex_shrink_0()
        .p(px(bezel))
        .rounded(px(corner + bezel))
        .bg(rgb(0x0c0c0e))
        .border(px(1.5))
        .border_color(rgb(0x4a4a4e))
        .child(screen)
        .children(buttons.into_iter().flatten())
}

fn area(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> AnyElement {
    let s = &view.simulator;
    if let Some(form) = s.adding.as_ref() {
        return new_simulator_form(form, a, cx).into_any_element();
    }
    let device = s.device();
    let image = s.annotation.as_ref().map(|n| n.image.clone()).or_else(|| {
        s.frame
            .as_ref()
            .filter(|(udid, _)| device.is_some_and(|d| &d.udid == udid))
            .map(|(_, image)| image.clone())
    });
    let room = s.room.clone();
    let measure = canvas(
        move |bounds, _, _| {
            room.set((
                f32::from(bounds.size.width) - 2.0 * AREA_PAD,
                f32::from(bounds.size.height) - 2.0 * AREA_PAD,
            ));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let body = div()
        .id("sim-area")
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .child(measure);
    let busy = s.busy.clone();
    if let (Some(image), Some(device), None) = (image, device, busy.as_ref()) {
        let zoomed = !matches!(s.zoom(), Zoom::Fit);
        let phone = phone(view, image, device, cx);
        let inner = div()
            .min_w(relative(1.0))
            .min_h(relative(1.0))
            .p(px(AREA_PAD))
            .flex()
            .items_center()
            .justify_center()
            .child(phone);
        let scroller = div()
            .id("sim-scroll")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .when(zoomed, |d| d.overflow_scroll())
            .child(inner);
        return body
            .child(scroller)
            .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.add_media(paths.paths().to_vec(), cx);
            }))
            .into_any_element();
    }
    let (line, offer) = match (&busy, s.listed, device) {
        (Some(b), _, _) => (b.clone(), false),
        (None, false, _) => ("Looking for a simulator…".to_owned(), false),
        (None, true, Some(_)) => ("Waiting for the screen…".to_owned(), false),
        (None, true, None) => ("No simulator is running".to_owned(), true),
    };
    body.child(
        div()
            .absolute()
            .size_full()
            .p(px(16.0))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .child(label(&line, TypeRole::Body, Ink::TextMuted, a))
            .when(offer, |b| {
                b.child(
                    button(
                        "sim-pick-empty",
                        "Choose a Simulator",
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.simulator.menu_open = true;
                        cx.notify();
                    })),
                )
            }),
    )
    .into_any_element()
}

fn divider(a: Appearance) -> Div {
    div()
        .w(px(1.0))
        .h(px(18.0))
        .mx(px(4.0))
        .fill(Ink::Hairline, a)
}

fn controls(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let s = &view.simulator;
    let has = s.device().is_some() && s.busy.is_none() && s.adding.is_none();
    let bar = div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .px(px(8.0))
        .py(px(4.0))
        .rounded_full()
        .fill(Ink::Raised, a)
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)));
    let bar = if s.annotation.is_some() {
        bar.child(
            button(
                "sim-ann-clear",
                "Clear",
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(n) = this.simulator.annotation.as_mut() {
                    n.strokes.clear();
                }
                cx.notify();
            })),
        )
        .child(
            button(
                "sim-ann-cancel",
                "Cancel",
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(|this, _, _, cx| this.end_annotation(cx))),
        )
        .child(
            button(
                "sim-ann-add",
                "Add to Reply",
                ButtonKind::Primary,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(|this, _, window, cx| this.add_annotation(window, cx))),
        )
    } else {
        let recording = s.recording.is_some();
        let framed = has && s.frame.is_some();
        bar.child(
            tool("sim-home", Icon::Home, "Home", false, has, a).when(has, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.press_simulator_key(SimKey::Home, cx)))
            }),
        )
        .child(
            tool("sim-rotate", Icon::Resume, "Rotate", false, has, a).when(has, |b| {
                b.on_click(
                    cx.listener(|this, _, _, cx| this.press_simulator_key(SimKey::Rotate, cx)),
                )
            }),
        )
        .child(divider(a))
        .child(
            tool(
                "sim-shot",
                Icon::Camera,
                "Save a screenshot to the Desktop",
                false,
                has,
                a,
            )
            .when(has, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.save_screenshot(cx)))
            }),
        )
        .child(
            tool(
                "sim-record",
                if recording { Icon::Stop } else { Icon::Video },
                if recording {
                    "Stop recording and save the video to the Desktop"
                } else {
                    "Record the screen to a video on the Desktop"
                },
                recording,
                has || recording,
                a,
            )
            .when(has || recording, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.toggle_recording(cx)))
            }),
        )
        .child(divider(a))
        .child(
            tool(
                "sim-annotate",
                Icon::Edit,
                "Annotate: draw on the screen and add it to the reply",
                false,
                framed,
                a,
            )
            .when(framed, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.begin_annotation(cx)))
            }),
        )
    };
    let note = s
        .note
        .as_ref()
        .map(|note| label(note, TypeRole::Meta, Ink::TextMuted, a).elide());
    div()
        .flex_shrink_0()
        .px(px(12.0))
        .pt(px(4.0))
        .pb(px(12.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .child(bar)
        .children(note)
}

pub fn render(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<AnyElement> {
    if !view.simulator.shown() || view.page.is_open() {
        return None;
    }
    let sidebar = view.sidebar_width();
    let available = f32::from(window.viewport_size().width) - sidebar;
    Some(
        div()
            .relative()
            .h_full()
            .w(px(panel_width(WIDTH, available)))
            .flex_shrink_0()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .fill(Ink::Window, a)
            .border_l(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(header(view, a, cx))
            .child(div().h(px(1.0)).flex_shrink_0().fill(Ink::Hairline, a))
            .child(area(view, a, cx))
            .child(controls(view, a, cx))
            .children(device_menu(view, a, cx))
            .window_region(a, cx)
            .part_at("Simulator Panel", Corner::TopTrailing, a, cx)
            .into_any_element(),
    )
}

pub fn button_in_header(
    view: &ChatView,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> impl IntoElement {
    let kind = if view.simulator.shown() {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    gate::tip_text(
        icon_button(
            "simulator-button",
            Icon::Simulator,
            IconSize::Button.points(),
            kind,
            PartState::Rest,
            a,
        )
        .size(px(crate::chat::view::sidebar::HEADER_BUTTON))
        .px(px(0.0)),
        SharedString::from("iOS Simulator: its screen, live"),
        a,
    )
    .on_click(cx.listener(|this, _, _, cx| this.toggle_simulator(cx)))
    .part_at("Simulator Button", Corner::BottomTrailing, a, cx)
}
