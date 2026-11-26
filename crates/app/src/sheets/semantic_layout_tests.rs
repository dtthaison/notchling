use gpui::{
    div, prelude::*, px, size, AnyView, Bounds, Context, Pixels, Render, TestAppContext,
    VisualTestContext, Window,
};

use crate::sheets::renders::VIEWS;
use crate::sheets::view::build_render;

struct Host(AnyView);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

const VIEWS_SHOWN: [&str; 4] = [
    "settings-semantic-models",
    "settings-semantic-standard",
    "settings-semantic-downloading",
    "settings-semantic-indexing",
];

const WIDTHS: [f32; 3] = [620.0, 480.0, 360.0];

fn found(cx: &mut VisualTestContext, name: &'static str) -> Bounds<Pixels> {
    cx.debug_bounds(name)
        .unwrap_or_else(|| panic!("{name} was not laid out"))
}

fn drawn<'a>(cx: &'a mut TestAppContext, view: &str, width: f32) -> &'a mut VisualTestContext {
    let name = view.to_owned();
    let (_, cx) = cx.add_window_view(move |_, cx| {
        Host(build_render(&name, cx).unwrap_or_else(|| panic!("{name} is not a render")))
    });
    cx.simulate_resize(size(px(width), px(1980.0)));
    cx.update(|window, cx| {
        window.draw(cx).clear();
    });
    cx
}

fn near(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() < 0.5
}

#[gpui::test]
fn every_model_row_keeps_its_button_inside_the_card(cx: &mut TestAppContext) {
    let selectors: [(&'static str, &'static str, &'static str); 2] = [
        ("semantic-row-0", "semantic-text-0", "semantic-button-0"),
        ("semantic-row-1", "semantic-text-1", "semantic-button-1"),
    ];
    for view in VIEWS_SHOWN {
        assert!(VIEWS.iter().any(|v| v.name == view), "{view}");
        for width in WIDTHS {
            let cx = drawn(cx, view, width);
            let block = found(cx, "semantic-block");
            for (row_name, text_name, button_name) in selectors {
                let row = found(cx, row_name);
                let text = found(cx, text_name);
                let button = found(cx, button_name);
                let what = format!("{view} at {width}: {row_name}");
                assert!(
                    button.right() <= block.right() + px(0.5),
                    "{what}: the button's right edge {:?} is past the card's {:?}",
                    button.right(),
                    block.right()
                );
                assert!(
                    near(button.right(), row.right()),
                    "{what}: the button's right edge {:?} is not the row's {:?}",
                    button.right(),
                    row.right()
                );
                assert!(
                    text.right() <= button.left(),
                    "{what}: the text reaches {:?}, the button starts at {:?}",
                    text.right(),
                    button.left()
                );
                assert!(
                    near(row.left(), block.left()),
                    "{what}: the row starts late"
                );
            }
        }
    }
}
