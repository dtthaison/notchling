//! Where Chat's window was last: its screen (the display's stable id) and its frame on that screen,
//! kept in the app's `settings.json` (`chatWindowFrame`), so it opens again the same size, on the
//! same screen, in the same place. Pure; `mac::window` reads and writes it.
//!
//! The frame is in the display's own coordinates, top-left origin, as GPUI gives and takes it. A
//! frame that no longer fits (the screen went, or got smaller) is pulled onto it, never left off
//! screen.

use serde_json::{json, Value};

/// The setting's key.
pub const KEY: &str = "chatWindowFrame";

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The display's stable id (its UUID); empty when it couldn't be read.
    pub display: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Zoomed (the green button): it opens zoomed again.
    pub maximized: bool,
}

impl Frame {
    pub fn to_value(&self) -> Value {
        json!({"display": self.display, "x": self.x, "y": self.y, "width": self.width,
               "height": self.height, "maximized": self.maximized})
    }

    /// The setting read back; `None` for anything without a size.
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

    /// The frame on a display of `screen` (width, height): at least `min`, at most the screen, and
    /// moved so all of it is on the screen.
    pub fn fit(&self, screen: (f32, f32), min: (f32, f32)) -> (f32, f32, f32, f32) {
        let width = self.width.max(min.0).min(screen.0);
        let height = self.height.max(min.1).min(screen.1);
        let x = self.x.clamp(0.0, (screen.0 - width).max(0.0));
        let y = self.y.clamp(0.0, (screen.1 - height).max(0.0));
        (x, y, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // A smaller screen now: no bigger than it, and moved onto it.
        assert_eq!(
            frame(1500.0, 600.0, 1600.0, 1000.0).fit((1440.0, 900.0), (720.0, 460.0)),
            (0.0, 0.0, 1440.0, 900.0)
        );
        assert_eq!(
            frame(1500.0, 600.0, 1400.0, 900.0).fit((1440.0, 900.0), (720.0, 460.0)),
            (40.0, 0.0, 1400.0, 900.0)
        );
        // Hanging off the right and bottom edges: moved in, size kept.
        assert_eq!(
            frame(2000.0, 1200.0, 1000.0, 700.0).fit((2560.0, 1440.0), (720.0, 460.0)),
            (1560.0, 740.0, 1000.0, 700.0)
        );
        // Below the minimum size: grown to it.
        assert_eq!(
            frame(-50.0, 10.0, 300.0, 200.0).fit((2560.0, 1440.0), (720.0, 460.0)),
            (0.0, 10.0, 720.0, 460.0)
        );
    }
}
