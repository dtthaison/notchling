//! The pairing QR code as a grid of modules, drawn by the view. The payload is the daemon's, encoded
//! as is (`bridge.pair.begin`); the phone app reads the same JSON the Swift app's QR carried.

use qrcode::{Color, QrCode};

/// Empty modules around the code that scanners need (the standard's quiet zone).
pub const QUIET_ZONE: usize = 4;

/// A square grid of modules, row by row; `true` is dark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Qr {
    width: usize,
    dark: Vec<bool>,
}

impl Qr {
    /// Encodes `payload`; the error says why it can't be drawn (too long for any QR version).
    pub fn new(payload: &str) -> Result<Qr, String> {
        let code = QrCode::new(payload.as_bytes()).map_err(|e| format!("QR code: {e}"))?;
        Ok(Qr {
            width: code.width(),
            dark: code
                .to_colors()
                .into_iter()
                .map(|c| c == Color::Dark)
                .collect(),
        })
    }

    /// Modules per side, without the quiet zone.
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.width && self.dark.get(y * self.width + x) == Some(&true)
    }

    /// The dark stretches of row `y` as `(start, length)`, so a row draws as a few bars rather than
    /// a quad per module.
    pub fn runs(&self, y: usize) -> Vec<(usize, usize)> {
        let mut runs = Vec::new();
        let mut start = None;
        for x in 0..=self.width {
            match (self.is_dark(x, y), start) {
                (true, None) => start = Some(x),
                (false, Some(from)) => {
                    runs.push((from, x - from));
                    start = None;
                }
                _ => {}
            }
        }
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_payload_is_version_one() {
        let qr = Qr::new("hi").unwrap();
        assert_eq!(qr.width(), 21);
    }

    #[test]
    fn the_three_finder_patterns_are_in_place() {
        let qr = Qr::new("{\"v\":1,\"code\":\"K7M4XQ2P\"}").unwrap();
        let w = qr.width();
        // The 7x7 finder: a dark ring, a light ring, a 3x3 dark centre. Top left, top right and
        // bottom left.
        for (ox, oy) in [(0, 0), (w - 7, 0), (0, w - 7)] {
            assert!(qr.is_dark(ox, oy) && qr.is_dark(ox + 6, oy + 6));
            assert!(!qr.is_dark(ox + 1, oy + 1) && !qr.is_dark(ox + 5, oy + 5));
            assert!(qr.is_dark(ox + 3, oy + 3));
        }
    }

    #[test]
    fn runs_cover_exactly_the_dark_modules() {
        let qr = Qr::new("pair me").unwrap();
        for y in 0..qr.width() {
            let mut from_runs = vec![false; qr.width()];
            for (start, len) in qr.runs(y) {
                assert!(len > 0);
                from_runs[start..start + len].fill(true);
            }
            let dark: Vec<bool> = (0..qr.width()).map(|x| qr.is_dark(x, y)).collect();
            assert_eq!(from_runs, dark, "row {y}");
        }
        // The top row starts with the finder's seven dark modules.
        assert_eq!(qr.runs(0).first(), Some(&(0, 7)));
    }

    #[test]
    fn outside_the_grid_is_light() {
        let qr = Qr::new("x").unwrap();
        assert!(!qr.is_dark(qr.width(), 0) && !qr.is_dark(0, qr.width()));
    }

    #[test]
    fn a_payload_too_long_for_any_version_is_an_error() {
        let long = "x".repeat(4000);
        assert!(Qr::new(&long).is_err());
    }

    #[test]
    fn the_same_payload_makes_the_same_code() {
        assert_eq!(Qr::new("same").unwrap(), Qr::new("same").unwrap());
        assert_ne!(Qr::new("same").unwrap(), Qr::new("other").unwrap());
    }
}
