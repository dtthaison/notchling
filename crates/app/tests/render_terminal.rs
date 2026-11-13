#![cfg(target_os = "macos")]

use std::fs::File;
use std::path::Path;
use std::process::Command;

use notchling_app::views;

struct Shot {
    width: usize,
    height: usize,
    step: usize,
    line: usize,
    pixels: Vec<u8>,
}

impl Shot {
    fn at(&self, x: usize, y: usize) -> [u8; 3] {
        let i = y * self.line + x * self.step;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]]
    }
}

fn luma(p: [u8; 3]) -> f32 {
    (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.0
}

fn render(how: &str) -> Shot {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/chat");
    let out = std::env::temp_dir().join(format!(
        "notchling-terminal-{}-{}.png",
        std::process::id(),
        how.replace(',', "-")
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat", "--out"])
        .arg(&out)
        .env("NOTCHLING_DATA_ROOT", &root)
        .env("NOTCHLING_RENDER_TERMINAL", how)
        .env("NOTCHLING_RENDER_APPEARANCE", "dark")
        .env("NOTCHLING_RENDER_FORCE_FRAMES", "1")
        .output()
        .expect("run notchling-app");
    assert!(
        output.status.success(),
        "exit {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let decoder = png::Decoder::new(File::open(&out).expect("the PNG exists"));
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG pixels");
    std::fs::remove_file(&out).ok();
    Shot {
        width: info.width as usize,
        height: info.height as usize,
        step: info.line_size / info.width as usize,
        line: info.line_size,
        pixels,
    }
}

fn is_cursor(p: [u8; 3]) -> bool {
    let [r, g, b] = p.map(i32::from);
    b > 190 && g > 120 && r < 140 && b - r > 80 && g > r + 30
}

struct Cursor {
    x: (usize, usize),
    y: (usize, usize),
}

fn find_cursor(shot: &Shot) -> Cursor {
    let scale = shot.width as f32 / views::spec("chat").expect("the view exists").size.0;
    let (from_x, from_y) = ((280.0 * scale) as usize, (80.0 * scale) as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in from_y..shot.height {
        for x in from_x..shot.width {
            if is_cursor(shot.at(x, y)) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    assert!(x1 > x0 && y1 > y0, "no cursor in the terminal panel");
    Cursor {
        x: (x0, x1 + 1),
        y: (y0, y1 + 1),
    }
}

struct Inside {
    marks: usize,
    strokes: usize,
}

fn inside(shot: &Shot, c: &Cursor) -> Inside {
    let margin = 3;
    let fill = luma(shot.at(c.x.0 + margin, c.y.0 + margin));
    let (mut marks, mut strokes) = (0, 0);
    for y in c.y.0 + margin..c.y.1 - margin {
        for x in c.x.0 + margin..c.x.1 - margin {
            let d = (luma(shot.at(x, y)) - fill).abs();
            if d > 0.03 {
                marks += 1;
            }
            if d > 0.35 {
                strokes += 1;
            }
        }
    }
    Inside { marks, strokes }
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn the_cursor_after_typed_text_covers_an_empty_cell() {
    let shot = render("typed,focus,expanded");
    let c = find_cursor(&shot);
    let inside = inside(&shot, &c);
    assert_eq!(
        inside.marks, 0,
        "the cursor covers {} pixels of a typed character",
        inside.marks
    );
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn the_cursor_over_a_character_shows_it_in_a_contrast_color() {
    let shot = render("over,focus,expanded");
    let c = find_cursor(&shot);
    let inside = inside(&shot, &c);
    assert!(
        inside.strokes >= 12,
        "only {} pixels of the character under the cursor contrast with it",
        inside.strokes
    );
}

#[test]
#[ignore = "opens a window and captures it; needs a window server"]
fn the_cursor_on_a_wide_character_is_two_cells_wide() {
    let shot = render("wide,focus,expanded");
    let c = find_cursor(&shot);
    let scale = shot.width as f32 / views::spec("chat").expect("the view exists").size.0;
    let cell = 12.0 * 0.602 * scale;
    let width = (c.x.1 - c.x.0) as f32;
    assert!(
        (width - 2.0 * cell).abs() <= 2.0,
        "the cursor is {width} px wide, two cells are {} px",
        2.0 * cell
    );
    let inside = inside(&shot, &c);
    assert!(
        inside.strokes >= 20,
        "only {} pixels of the wide character contrast with the cursor",
        inside.strokes
    );
}
