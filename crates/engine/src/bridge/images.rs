//! The JPEG a chat picture reaches the phone as (the `jpeg` half of ConsoleCore's `ChatImages`,
//! which used ImageIO). The bytes themselves come from `transcript::chat::images`.

use image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, ImageDecoder, ImageReader,
    Limits, RgbImage,
};
use std::io::Cursor;

/// Pictures are encoded at this quality.
const QUALITY: u8 = 80;
/// The longest side asked for is kept between these.
const MIN_SIDE: u32 = 16;
const MAX_SIDE: u32 = 1600;
/// A picture claiming more pixels than this is refused before it is decoded.
const MAX_PIXELS: u64 = 100_000_000;
/// Memory the decoder may allocate.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImagingError {
    /// Not a picture, or one that is damaged or too big.
    Unreadable,
    /// A format this computer cannot turn into a JPEG (HEIC).
    Unsupported(String),
}

/// Turns the bytes of any picture into a JPEG for the phone.
pub trait Imaging: Send + Sync {
    /// At most `max_width` pixels on the longer side (16 to 1600), never enlarged, flattened onto
    /// white because a JPEG has no transparency.
    fn jpeg(&self, data: &[u8], max_width: i64) -> Result<Vec<u8>, ImagingError>;
}

/// PNG, JPEG, GIF and WebP through the `image` crate.
pub struct ImageCrate;

impl Imaging for ImageCrate {
    fn jpeg(&self, data: &[u8], max_width: i64) -> Result<Vec<u8>, ImagingError> {
        if data.len() >= 12 && &data[4..8] == b"ftyp" {
            return Err(ImagingError::Unsupported(
                "ảnh HEIC chưa đổi được sang JPEG trên máy này".into(),
            ));
        }
        let side = max_width.clamp(i64::from(MIN_SIDE), i64::from(MAX_SIDE)) as u32;
        let mut reader = ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|_| ImagingError::Unreadable)?;
        let mut limits = Limits::default();
        limits.max_alloc = Some(MAX_ALLOC);
        reader.limits(limits);
        let decoder = reader
            .into_decoder()
            .map_err(|_| ImagingError::Unreadable)?;
        let (width, height) = decoder.dimensions();
        if u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err(ImagingError::Unreadable);
        }
        let picture = DynamicImage::from_decoder(decoder).map_err(|_| ImagingError::Unreadable)?;
        let picture = if picture.width().max(picture.height()) > side {
            picture.resize(side, side, FilterType::Triangle)
        } else {
            picture
        };
        let flat = flatten(&picture);
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, QUALITY)
            .encode_image(&flat)
            .map_err(|_| ImagingError::Unreadable)?;
        Ok(out)
    }
}

/// The picture over white: each channel `c * a + 255 * (1 - a)`.
fn flatten(picture: &DynamicImage) -> RgbImage {
    let rgba = picture.to_rgba8();
    let mut flat = RgbImage::new(rgba.width(), rgba.height());
    for (target, source) in flat.pixels_mut().zip(rgba.pixels()) {
        let alpha = u32::from(source[3]);
        for channel in 0..3 {
            let value = u32::from(source[channel]) * alpha + 255 * (255 - alpha);
            target[channel] = (value / 255) as u8;
        }
    }
    flat
}
