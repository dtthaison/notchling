use super::*;
use image::{codecs::png::PngEncoder, ImageEncoder, RgbImage};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

fn noisy_png(width: u32, height: u32) -> Vec<u8> {
    let mut seed = 0x9e37_79b9_u32;
    let mut picture = RgbImage::new(width, height);
    for pixel in picture.pixels_mut() {
        for channel in &mut pixel.0 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *channel = (seed >> 24) as u8;
        }
    }
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(
            picture.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("encodes");
    out
}

fn scratch(text: &[u8]) -> (String, std::path::PathBuf) {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("shot-unit-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("a.jsonl");
    fs::write(&path, text).expect("write");
    (path.to_string_lossy().into_owned(), dir)
}

fn result_line(id: &str, png: &[u8]) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    format!(
        r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":[{{"type":"text","text":"Successfully captured screenshot"}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{data}"}}}}]}}]}}}}"#
    )
}

#[test]
fn a_line_is_read_from_its_offset_up_to_the_newline() {
    let (path, dir) = scratch(b"first\nsecond\nthird");
    assert_eq!(read_line(&path, 6, 100), Some(b"second".to_vec()));
    assert_eq!(read_line(&path, 13, 100), Some(b"third".to_vec()));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_line_over_the_cap_is_refused() {
    let (path, dir) = scratch(&[b'x'; 50]);
    assert_eq!(read_line(&path, 0, 49), None);
    assert!(read_line(&path, 0, 50).is_some());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_nth_image_of_the_right_result_is_found() {
    let line = br#"{"x":[{"tool_use_id":"c1","content":[{"type":"text","text":"a"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"BBBB"}}]}]}"#;
    assert_eq!(image_data(line, "c1", 0), Some(&b"AAAA"[..]));
    assert_eq!(image_data(line, "c1", 1), Some(&b"BBBB"[..]));
    assert_eq!(image_data(line, "c1", 2), None);
    assert_eq!(image_data(line, "other", 0), None);
}

#[test]
fn a_big_screenshot_becomes_a_small_jpeg() {
    let png = noisy_png(1200, 800);
    let line = result_line("c1", &png);
    assert!(line.len() > 400_000, "{}", line.len());
    let (path, dir) = scratch(format!("{{}}\n{line}\n").as_bytes());
    let spot = ShotSpot {
        path,
        at: 3,
        index: 0,
    };
    let thumb = thumbnail(&spot, "c1").expect("thumbnail");
    assert_eq!(&thumb[..2], [0xFF, 0xD8]);
    assert!(thumb.len() < png.len(), "{} {}", thumb.len(), png.len());
    let decoded = image::load_from_memory(&thumb).expect("decodes");
    assert_eq!(decoded.width(), 640);
    assert_eq!(decoded.height(), 427);
    assert!(thumbnail(&spot, "other").is_none());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn garbage_gives_no_thumbnail() {
    let (path, dir) = scratch(
        br#"{"tool_use_id":"c1","content":[{"type":"image","source":{"data":"!!!notbase64"}}]}"#,
    );
    let spot = ShotSpot {
        path,
        at: 0,
        index: 0,
    };
    assert!(thumbnail(&spot, "c1").is_none());
    let _ = fs::remove_dir_all(dir);
}
