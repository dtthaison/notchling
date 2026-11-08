//! The JPEG a chat picture is sent as (Swift `ChatImages.jpeg`, tested there on a noise PNG).

use super::images::{ImageCrate, Imaging, ImagingError};
use image::{codecs::png::PngEncoder, ImageEncoder, Rgba, RgbaImage};

fn noise_png(width: u32, height: u32) -> Vec<u8> {
    let mut seed: u64 = 42;
    let picture = RgbaImage::from_fn(width, height, |_, _| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let bits = (seed >> 33) as u8;
        Rgba([bits, bits.wrapping_mul(3), bits.wrapping_add(90), 255])
    });
    encode(&picture)
}

fn encode(picture: &RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(
            picture.as_raw(),
            picture.width(),
            picture.height(),
            image::ExtendedColorType::Rgba8,
        )
        .expect("png");
    bytes
}

fn size_of_jpeg(jpeg: &[u8]) -> (u32, u32) {
    let decoded = image::load_from_memory(jpeg).expect("a JPEG");
    (decoded.width(), decoded.height())
}

#[test]
fn a_big_screenshot_becomes_a_jpeg_no_wider_than_asked() {
    let jpeg = ImageCrate.jpeg(&noise_png(300, 200), 100).expect("jpeg");
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "a JPEG");
    let (width, height) = size_of_jpeg(&jpeg);
    assert_eq!(width, 100);
    assert!((60..=100).contains(&height), "aspect kept: {height}");
}

#[test]
fn a_tall_picture_is_bounded_by_its_longer_side() {
    let (width, height) = size_of_jpeg(&ImageCrate.jpeg(&noise_png(100, 400), 200).expect("jpeg"));
    assert_eq!(height, 200);
    assert_eq!(width, 50);
}

#[test]
fn a_small_picture_is_not_enlarged_and_the_limit_is_clamped() {
    let small = noise_png(40, 30);
    assert_eq!(
        size_of_jpeg(&ImageCrate.jpeg(&small, 800).expect("jpeg")),
        (40, 30)
    );
    // Asking for 1 pixel gives the 16-pixel floor; asking for 100 000 gives the 1600 ceiling.
    assert_eq!(
        size_of_jpeg(&ImageCrate.jpeg(&small, 1).expect("jpeg")),
        (16, 12)
    );
    let wide = noise_png(2000, 100);
    assert_eq!(
        size_of_jpeg(&ImageCrate.jpeg(&wide, 100_000).expect("jpeg")).0,
        1600
    );
}

#[test]
fn a_transparent_picture_is_flattened_onto_white() {
    // JPEG has no transparency: without flattening a transparent screenshot turns black.
    let clear = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 0]));
    let jpeg = ImageCrate.jpeg(&encode(&clear), 32).expect("jpeg");
    let decoded = image::load_from_memory(&jpeg).expect("jpeg").to_rgb8();
    let pixel = decoded.get_pixel(16, 16);
    assert!(pixel.0.iter().all(|c| *c > 240), "white, got {pixel:?}");
}

#[test]
fn bytes_that_are_not_a_picture_are_unreadable() {
    assert_eq!(
        ImageCrate.jpeg(b"not an image", 100),
        Err(ImagingError::Unreadable)
    );
    assert_eq!(ImageCrate.jpeg(&[], 100), Err(ImagingError::Unreadable));
}

#[test]
fn a_picture_claiming_a_huge_size_is_refused_before_it_is_decoded() {
    // A PNG header that says 60 000 x 60 000 (3.6 billion pixels) and holds no pixels.
    let mut header = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    header.extend_from_slice(b"IHDR");
    header.extend_from_slice(&60_000u32.to_be_bytes());
    header.extend_from_slice(&60_000u32.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    assert!(ImageCrate.jpeg(&header, 100).is_err());
}

#[test]
fn heic_is_not_decoded_here() {
    let mut heic = vec![0, 0, 0, 24];
    heic.extend_from_slice(b"ftypheic");
    heic.extend_from_slice(&[0; 12]);
    assert!(matches!(
        ImageCrate.jpeg(&heic, 100),
        Err(ImagingError::Unsupported(_)) | Err(ImagingError::Unreadable)
    ));
}
