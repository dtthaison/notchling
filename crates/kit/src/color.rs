#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Color {
    pub const fn hex(rgb: u32) -> Color {
        Color {
            r: ((rgb >> 16) & 0xFF) as u8,
            g: ((rgb >> 8) & 0xFF) as u8,
            b: (rgb & 0xFF) as u8,
            a: 1.0,
        }
    }

    pub const fn with_alpha(self, a: f32) -> Color {
        Color {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    pub const BLACK: Color = Color::hex(0x000000);
    pub const WHITE: Color = Color::hex(0xFFFFFF);
    pub const CLEAR: Color = Color::hex(0x000000).with_alpha(0.0);

    pub fn channels(self) -> [f32; 4] {
        [
            f32::from(self.r) / 255.0,
            f32::from(self.g) / 255.0,
            f32::from(self.b) / 255.0,
            self.a,
        ]
    }

    pub fn hex_string(self) -> String {
        if (self.a - 1.0).abs() < f32::EPSILON {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            let a = (self.a.clamp(0.0, 1.0) * 255.0).round() as u8;
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, a)
        }
    }

    pub fn over(self, below: Color) -> Color {
        let a = self.a.clamp(0.0, 1.0);
        let mix = |top: u8, under: u8| -> u8 {
            (f32::from(top) * a + f32::from(under) * (1.0 - a)).round() as u8
        };
        Color {
            r: mix(self.r, below.r),
            g: mix(self.g, below.g),
            b: mix(self.b, below.b),
            a: 1.0,
        }
    }

    pub fn luminance(self) -> f32 {
        let lin = |c: u8| {
            let v = f32::from(c) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    pub fn contrast(self, other: Color) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_splits_channels() {
        let c = Color::hex(0xF7873B);
        assert_eq!((c.r, c.g, c.b), (0xF7, 0x87, 0x3B));
        assert_eq!(c.a, 1.0);
        assert_eq!(c.hex_string(), "#F7873B");
    }

    #[test]
    fn alpha_shows_in_the_hex_string() {
        assert_eq!(
            Color::hex(0xF7873B).with_alpha(0.5).hex_string(),
            "#F7873B80"
        );
        assert_eq!(Color::CLEAR.hex_string(), "#00000000");
    }

    #[test]
    fn channels_are_unit_floats() {
        let [r, g, b, a] = Color::hex(0xFF0080).with_alpha(0.25).channels();
        assert_eq!((r, b, a), (1.0, 128.0 / 255.0, 0.25));
        assert_eq!(g, 0.0);
    }

    #[test]
    fn over_blends_by_alpha() {
        let white_half = Color::WHITE.with_alpha(0.5);
        assert_eq!(white_half.over(Color::BLACK), Color::hex(0x808080));
        assert_eq!(
            Color::hex(0x123456).over(Color::WHITE),
            Color::hex(0x123456)
        );
        assert_eq!(
            Color::CLEAR.over(Color::hex(0x123456)),
            Color::hex(0x123456)
        );
    }

    #[test]
    fn contrast_black_white_is_21() {
        assert!((Color::BLACK.contrast(Color::WHITE) - 21.0).abs() < 0.01);
        assert!((Color::WHITE.contrast(Color::WHITE) - 1.0).abs() < 0.01);
    }
}
