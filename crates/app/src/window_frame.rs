use std::path::Path;

use notchling_engine::settings::Settings;
use serde_json::{json, Value};

pub const KEY: &str = "chatWindowFrame";

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub display: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl Frame {
    pub fn to_value(&self) -> Value {
        json!({"display": self.display, "x": self.x, "y": self.y, "width": self.width,
               "height": self.height, "maximized": self.maximized})
    }

    pub fn from_value(value: Option<&Value>) -> Option<Frame> {
        let v = value?;
        let num = |k: &str| v.get(k).and_then(Value::as_f64).map(|n| n as f32);
        let frame = Frame {
            display: v
                .get("display")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            x: num("x").unwrap_or(0.0),
            y: num("y").unwrap_or(0.0),
            width: num("width")?,
            height: num("height")?,
            maximized: v.get("maximized").and_then(Value::as_bool).unwrap_or(false),
        };
        (frame.width > 0.0 && frame.height > 0.0).then_some(frame)
    }

    pub fn fit(&self, screen: (f32, f32), min: (f32, f32)) -> (f32, f32, f32, f32) {
        let width = self.width.max(min.0).min(screen.0);
        let height = self.height.max(min.1).min(screen.1);
        let x = self.x.clamp(0.0, (screen.0 - width).max(0.0));
        let y = self.y.clamp(0.0, (screen.1 - height).max(0.0));
        (x, y, width, height)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub id: String,
    pub size: (f32, f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub screen: usize,
    pub rect: (f32, f32, f32, f32),
    pub maximized: bool,
}

pub fn place(
    saved: Option<&Frame>,
    screens: &[Screen],
    main: usize,
    default: (f32, f32),
    min: (f32, f32),
) -> Option<Placed> {
    let main = main.min(screens.len().checked_sub(1)?);
    if let Some(saved) = saved {
        if let Some(i) = screens
            .iter()
            .position(|s| !saved.display.is_empty() && s.id == saved.display)
        {
            return Some(Placed {
                screen: i,
                rect: saved.fit(screens[i].size, min),
                maximized: saved.maximized,
            });
        }
    }
    let size = saved.map_or(default, |f| (f.width, f.height));
    let screen = screens[main].size;
    let (w, h) = (
        size.0.max(min.0).min(screen.0),
        size.1.max(min.1).min(screen.1),
    );
    Some(Placed {
        screen: main,
        rect: ((screen.0 - w) / 2.0, (screen.1 - h) / 2.0, w, h),
        maximized: false,
    })
}

pub fn load(settings: &Path) -> Option<Frame> {
    let store = Settings::open(settings).ok()?;
    Frame::from_value(store.get(KEY).as_ref())
}

pub fn save(settings: &Path, frame: &Frame) -> Result<(), String> {
    let mut store = Settings::open(settings).map_err(|e| e.to_string())?;
    store.set(KEY, frame.to_value()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Scratch;

    fn screens() -> Vec<Screen> {
        vec![
            Screen {
                id: "MAIN".into(),
                size: (1440.0, 900.0),
            },
            Screen {
                id: "D1".into(),
                size: (2560.0, 1440.0),
            },
        ]
    }

    const MIN: (f32, f32) = (720.0, 460.0);
    const DEFAULT: (f32, f32) = (1040.0, 720.0);

    #[test]
    fn a_connected_display_gets_the_same_size_at_the_same_place() {
        let p = place(
            Some(&frame(300.0, 80.0, 1400.0, 900.0)),
            &screens(),
            0,
            DEFAULT,
            MIN,
        );
        assert_eq!(
            p,
            Some(Placed {
                screen: 1,
                rect: (300.0, 80.0, 1400.0, 900.0),
                maximized: false
            })
        );
    }

    #[test]
    fn a_display_that_is_gone_gets_the_same_size_centered_on_the_main_one() {
        let mut f = frame(300.0, 80.0, 1000.0, 700.0);
        f.display = "UNPLUGGED".into();
        let p = place(Some(&f), &screens(), 0, DEFAULT, MIN).expect("placed");
        assert_eq!((p.screen, p.rect), (0, (220.0, 100.0, 1000.0, 700.0)));
        f.width = 2400.0;
        f.height = 1300.0;
        let p = place(Some(&f), &screens(), 0, DEFAULT, MIN).expect("placed");
        assert_eq!((p.screen, p.rect), (0, (0.0, 0.0, 1440.0, 900.0)));
    }

    #[test]
    fn a_frame_without_a_display_id_is_treated_as_a_missing_display() {
        let mut f = frame(300.0, 80.0, 1000.0, 700.0);
        f.display = String::new();
        let p = place(Some(&f), &screens(), 0, DEFAULT, MIN).expect("placed");
        assert_eq!(p.screen, 0);
    }

    #[test]
    fn a_display_that_got_smaller_pulls_the_window_in() {
        let mut small = screens();
        small[1].size = (1280.0, 720.0);
        let p = place(
            Some(&frame(1500.0, 600.0, 1400.0, 900.0)),
            &small,
            0,
            DEFAULT,
            MIN,
        )
        .expect("placed");
        assert_eq!((p.screen, p.rect), (1, (0.0, 0.0, 1280.0, 720.0)));
    }

    #[test]
    fn a_frame_mostly_off_its_display_comes_back_whole() {
        let p = place(
            Some(&frame(2400.0, 1300.0, 1000.0, 700.0)),
            &screens(),
            0,
            DEFAULT,
            MIN,
        )
        .expect("placed");
        assert_eq!(p.rect, (1560.0, 740.0, 1000.0, 700.0));
    }

    #[test]
    fn nothing_saved_opens_the_default_size_centered_on_the_main_display() {
        let p = place(None, &screens(), 0, DEFAULT, MIN).expect("placed");
        assert_eq!((p.screen, p.rect), (0, (200.0, 90.0, 1040.0, 720.0)));
        assert_eq!(place(None, &[], 0, DEFAULT, MIN), None);
    }

    #[test]
    fn a_saved_frame_lands_in_the_store_and_reads_back() {
        let dir = Scratch::new("frame-save");
        let file = dir.path().join("settings.json");
        let f = frame(120.0, 60.0, 1300.0, 840.0);
        save(&file, &f).expect("the frame is saved");
        assert_eq!(load(&file), Some(f));
    }

    fn frame(x: f32, y: f32, width: f32, height: f32) -> Frame {
        Frame {
            display: "D1".into(),
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    #[test]
    fn the_setting_reads_back_what_was_written() {
        let f = Frame {
            maximized: true,
            ..frame(100.0, 40.0, 1400.0, 900.0)
        };
        assert_eq!(Frame::from_value(Some(&f.to_value())), Some(f));
        assert_eq!(Frame::from_value(None), None);
        assert_eq!(Frame::from_value(Some(&json!({"x": 1}))), None);
        assert_eq!(
            Frame::from_value(Some(&json!({"width": 0, "height": 10}))),
            None
        );
    }

    #[test]
    fn a_frame_that_fits_stays_where_it_was() {
        assert_eq!(
            frame(300.0, 80.0, 1400.0, 900.0).fit((2560.0, 1440.0), (720.0, 460.0)),
            (300.0, 80.0, 1400.0, 900.0)
        );
    }

    #[test]
    fn a_frame_too_big_or_off_the_screen_is_pulled_onto_it() {
        assert_eq!(
            frame(1500.0, 600.0, 1600.0, 1000.0).fit((1440.0, 900.0), (720.0, 460.0)),
            (0.0, 0.0, 1440.0, 900.0)
        );
        assert_eq!(
            frame(1500.0, 600.0, 1400.0, 900.0).fit((1440.0, 900.0), (720.0, 460.0)),
            (40.0, 0.0, 1400.0, 900.0)
        );
        assert_eq!(
            frame(2000.0, 1200.0, 1000.0, 700.0).fit((2560.0, 1440.0), (720.0, 460.0)),
            (1560.0, 740.0, 1000.0, 700.0)
        );
        assert_eq!(
            frame(-50.0, 10.0, 300.0, 200.0).fit((2560.0, 1440.0), (720.0, 460.0)),
            (0.0, 10.0, 720.0, 460.0)
        );
    }
}
