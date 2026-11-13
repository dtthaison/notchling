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
