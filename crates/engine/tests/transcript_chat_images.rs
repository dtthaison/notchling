#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

mod common;

use base64::Engine;
use common::*;
use notchling_engine::transcript::chat::images::{self, ImageError, ImageOrigin};
use notchling_engine::transcript::chat::{self, ChatMessage, ChatRole};
use std::path::Path;

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

fn png(width: u32, height: u32, total: usize) -> Vec<u8> {
    let mut data = PNG_SIGNATURE.to_vec();
    data.extend_from_slice(&13u32.to_be_bytes());
    data.extend_from_slice(b"IHDR");
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    let mut seed: u64 = 42;
    while data.len() < total {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        data.push((seed >> 33) as u8);
    }
    data
}

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn small_png(width: u32, height: u32) -> String {
    b64(&png(width, height, 64))
}

fn image_block(base64: &str) -> String {
    format!(
        r#"{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{base64}"}}}}"#
    )
}

fn user_line(n: usize, text: Option<&str>, pictures: &[String]) -> String {
    let mut blocks: Vec<String> = pictures.iter().map(|p| image_block(p)).collect();
    if let Some(text) = text {
        blocks.push(format!(r#"{{"type":"text","text":"{text}"}}"#));
    }
    format!(
        r#"{{"type":"user","uuid":"u{n}","timestamp":"2026-11-03T08:00:{n:02}.000Z","message":{{"role":"user","content":[{}]}}}}"#,
        blocks.join(",")
    )
}

fn tool_result_line(n: usize, base64: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"r{n}","timestamp":"2026-11-03T08:00:{n:02}.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x","content":[{},{{"type":"text","text":"ok"}}]}}]}}}}"#,
        image_block(base64)
    )
}

fn tool_use_line(n: usize) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"t{n}","timestamp":"2026-11-03T08:00:{n:02}.000Z","message":{{"role":"assistant","model":"m","content":[{{"type":"tool_use","id":"x","name":"Read","input":{{"file_path":"/a.png"}}}}]}}}}"#
    )
}

fn parse(lines: &[String]) -> Vec<ChatMessage> {
    chat::parse(lines, true, "x", chat::DEFAULT_LIMIT).messages
}

#[test]
fn a_pasted_picture_belongs_to_the_user_message_and_never_carries_its_bytes() {
    let small = small_png(40, 30);
    let messages = parse(&[user_line(
        1,
        Some("look at this"),
        std::slice::from_ref(&small),
    )]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "look at this");
    let picture = &messages[0].images[0];
    assert_eq!(picture.media_type, "image/png");
    assert_eq!(picture.origin, ImageOrigin::User);
    assert_eq!((picture.width, picture.height), (Some(40), Some(30)));
    assert_eq!(picture.block_index, Some(0));
    assert_eq!(picture.id, "b-0-0");
    assert!(
        !format!("{:?}", messages[0]).contains(&small[..40]),
        "the message has no image bytes"
    );
}

#[test]
fn a_picture_with_no_text_is_still_a_message() {
    let messages = parse(&[user_line(1, None, &[small_png(8, 8)])]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "");
    assert_eq!(messages[0].images.len(), 1);
}

#[test]
fn a_tools_screenshot_goes_under_its_tool_row() {
    let messages = parse(&[tool_use_line(1), tool_result_line(2, &small_png(20, 10))]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, ChatRole::Tools);
    assert_eq!(messages[0].images[0].origin, ImageOrigin::ToolResult);
    assert_eq!(messages[0].images[0].width, Some(20));
}

#[test]
fn a_line_over_the_size_cap_is_not_dropped_when_the_bulk_is_a_picture() {
    let big = b64(&png(500, 500, 400_000));
    let line = user_line(1, Some("a big screenshot"), std::slice::from_ref(&big));
    assert!(line.len() > 300_000);
    let messages = parse(&[line, user_line(2, Some("next"), &[])]);
    assert!(messages[0].text.starts_with("a big screenshot"));
    let picture = &messages[0].images[0];
    assert_eq!(picture.width, Some(500));
    assert!((picture.byte_count - (big.len() * 3 / 4) as i64).abs() <= 3);
}

#[test]
fn a_long_line_without_pictures_is_still_skipped() {
    let junk = "x".repeat(100_010);
    let line = format!(
        r#"{{"type":"user","uuid":"u","timestamp":"2026-11-03T08:00:01.000Z","message":{{"role":"user","content":"{junk}"}}}}"#
    );
    assert!(parse(&[line]).is_empty());
}

#[test]
fn a_picture_in_a_queued_message() {
    let attachment = format!(
        r#"{{"type":"attachment","uuid":"q1","timestamp":"2026-11-03T08:00:05.000Z","attachment":{{"type":"queued_command","prompt":[{},{{"type":"text","text":"see"}}],"origin":{{"kind":"human"}}}}}}"#,
        image_block(&small_png(9, 9))
    );
    let messages = parse(&[attachment]);
    assert_eq!(messages[0].text, "see");
    assert_eq!(messages[0].images.len(), 1);
}

#[test]
fn two_pictures_in_one_line_keep_their_own_sizes_and_order() {
    let line = user_line(1, Some("two"), &[small_png(10, 20), small_png(30, 40)]);
    let messages = parse(&[line]);
    let sizes: Vec<_> = messages[0]
        .images
        .iter()
        .map(|i| (i.id.as_str(), i.width, i.height))
        .collect();
    assert_eq!(
        sizes,
        [("b-0-0", Some(10), Some(20)), ("b-0-1", Some(30), Some(40))]
    );
}

fn scratch_file(dir: &Path, name: &str, data: &[u8]) -> String {
    std::fs::create_dir_all(dir).expect("mkdir");
    let path = dir.join(name);
    std::fs::write(&path, data).expect("write");
    path.to_string_lossy().into_owned()
}

#[cfg(unix)]
struct TmpFolder(std::path::PathBuf);

#[cfg(unix)]
impl TmpFolder {
    fn new(tag: &str) -> TmpFolder {
        let folder = Path::new("/tmp").join(format!("mk-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("mkdir");
        TmpFolder(folder)
    }
}

#[cfg(unix)]
impl Drop for TmpFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn marker_path_rules() {
    let scratch = Scratch::new("marker-rules");
    let home = scratch.path("home");
    std::fs::create_dir_all(&home).expect("home");
    let in_home = scratch_file(&home.join("Pictures"), "shot.png", &[1, 2, 3]);
    let wrong_type = scratch_file(&home, "notes.txt", &[1, 2, 3]);
    let outside = scratch_file(&scratch.path("outside"), "out.png", &[1, 2, 3]);
    let directory = home.join("dir.png");
    std::fs::create_dir_all(&directory).expect("dir");
    let missing = home.join("missing.png");

    assert!(images::is_allowed_file(&in_home, &home));
    assert!(
        !images::is_allowed_file(&wrong_type, &home),
        "not an image extension"
    );
    if !scratch.dir.starts_with("/tmp") && !scratch.dir.starts_with("/private/tmp") {
        assert!(
            !images::is_allowed_file(&outside, &home),
            "outside home and /tmp"
        );
    }
    assert!(
        !images::is_allowed_file(&directory.to_string_lossy(), &home),
        "a folder is not a file"
    );
    assert!(!images::is_allowed_file(&missing.to_string_lossy(), &home));
    assert!(!images::is_allowed_file("relative/x.png", &home));
    let climbing = format!("{}/../../etc/x.png", home.to_string_lossy());
    assert!(
        !images::is_allowed_file(&climbing, &home),
        "dot-dot does not climb out"
    );
}

#[cfg(unix)]
#[test]
fn a_link_is_judged_by_where_it_leads() {
    let scratch = Scratch::new("marker-link");
    let home = scratch.path("home");
    std::fs::create_dir_all(&home).expect("home");
    let link = home.join("link.png");
    std::os::unix::fs::symlink("/etc/hosts", &link).expect("link");
    assert!(!images::is_allowed_file(&link.to_string_lossy(), &home));
    let tmp = TmpFolder::new("tmp-rule");
    let path = scratch_file(&tmp.0, "tmp.JPG", &[1, 2, 3]);
    assert!(images::is_allowed_file(&path, &home));
}

#[cfg(unix)]
#[test]
fn markers_become_references_only_for_allowed_files() {
    let home = Scratch::new("marker-home");
    let tmp = TmpFolder::new("markers");
    let path = scratch_file(&tmp.0, "pasted.png", &[1, 2, 3]);
    let text = format!("before [Image: source: {path}] after [Image: source: /etc/passwd.png]");
    let (rest, refs) = images::file_markers(&text, &home.dir);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].origin, ImageOrigin::File);
    assert_eq!(refs[0].path.as_deref(), Some(path.as_str()));
    assert!(
        rest.contains("[Image: source: /etc/passwd.png]"),
        "a refused marker stays as written"
    );
    assert!(!rest.contains(&path));
    assert!(refs[0].id.starts_with("f-") && refs[0].id.len() == 18);
}

#[cfg(unix)]
#[test]
fn a_file_marker_carries_the_pictures_real_size() {
    let home = Scratch::new("marker-size");
    let tmp = TmpFolder::new("size");
    let path = scratch_file(&tmp.0, "tall.png", &png(30, 65, 200));
    let (_, refs) = images::file_markers(&format!("[Image: source: {path}]"), &home.dir);
    assert_eq!((refs[0].width, refs[0].height), (Some(30), Some(65)));
    assert_eq!(refs[0].byte_count, 200);
    assert_eq!(refs[0].media_type, "image/png");
}

#[cfg(unix)]
#[test]
fn a_file_marker_in_a_message_becomes_the_messages_picture() {
    let tmp = TmpFolder::new("in-message");
    let path = scratch_file(&tmp.0, "note.png", &png(64, 64, 100));
    let line = format!(
        r#"{{"type":"user","uuid":"u1","timestamp":"2026-11-03T08:00:01.000Z","message":{{"role":"user","content":"see [Image: source: {path}]"}}}}"#
    );
    let messages = parse(&[line]);
    assert_eq!(messages[0].text, "see");
    assert_eq!(messages[0].images[0].origin, ImageOrigin::File);
}

#[test]
fn a_transcript_picture_is_read_again_from_its_line() {
    let scratch = Scratch::new("data-block");
    let source = png(300, 200, 5000);
    let lines = [
        user_line(1, Some("first"), &[]),
        user_line(2, Some("hi"), &[b64(&source)]),
    ];
    let path = s(&scratch.write("t.jsonl", &(lines.join("\n") + "\n")));
    let read = chat::read(&path, true, 10);
    let picture = read
        .messages
        .iter()
        .find_map(|m| m.images.first())
        .expect("picture");
    assert_eq!(picture.id, format!("b-{}-0", lines[0].len() + 1));
    let bytes = images::data(picture, Path::new(&path), &scratch.dir).expect("bytes");
    assert_eq!(bytes, source);
}

#[test]
fn a_reference_to_a_line_that_is_not_a_picture_finds_nothing() {
    let scratch = Scratch::new("data-missing");
    let path = scratch.write("t.jsonl", &(user_line(1, Some("hi"), &[]) + "\n"));
    let mut picture = images::ImageRef::for_block(0, 3, "image/png");
    assert_eq!(
        images::data(&picture, &path, &scratch.dir),
        Err(ImageError::NotFound)
    );
    picture.block_index = None;
    assert_eq!(
        images::data(&picture, &path, &scratch.dir),
        Err(ImageError::NotFound)
    );
}

#[cfg(unix)]
#[test]
fn a_file_over_the_cap_is_refused() {
    let home = Scratch::new("data-huge");
    let tmp = TmpFolder::new("huge");
    let path = scratch_file(&tmp.0, "huge.png", &png(64, 64, 100));
    let (_, refs) = images::file_markers(&format!("[Image: source: {path}]"), &home.dir);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open")
        .set_len(images::MAX_FILE_BYTES + 1)
        .expect("grow");
    assert_eq!(
        images::data(&refs[0], &home.path("t.jsonl"), &home.dir).map(|d| d.len()),
        Err(ImageError::TooLarge)
    );
}

#[cfg(unix)]
#[test]
fn a_fifo_swapped_in_after_the_check_is_refused_without_waiting() {
    let home = Scratch::new("data-fifo");
    let tmp = TmpFolder::new("fifo");
    let path = scratch_file(&tmp.0, "pipe.png", &png(64, 64, 100));
    let resolved = images::allowed_file(&path, &home.dir).expect("allowed");
    std::fs::remove_file(&resolved).expect("remove");
    let name = std::ffi::CString::new(resolved.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: a valid C string.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    let reading = resolved.clone();
    std::thread::spawn(move || {
        let _ = tx.send(images::read_allowed(&reading));
    });
    let answer = rx.recv_timeout(std::time::Duration::from_secs(2));
    if answer.is_err() {
        let _ = std::fs::OpenOptions::new().write(true).open(&resolved);
    }
    assert_eq!(
        answer.expect("answered without waiting for a writer"),
        Err(ImageError::NotAllowed)
    );
}

#[cfg(unix)]
#[test]
fn a_link_swapped_in_after_the_check_is_not_followed() {
    let home = Scratch::new("data-swap");
    let secret = scratch_file(&home.path("outside"), "secret.png", b"secret");
    let tmp = TmpFolder::new("swap");
    let path = scratch_file(&tmp.0, "note.png", &png(64, 64, 100));
    let resolved = images::allowed_file(&path, &home.path("nowhere")).expect("allowed");
    std::fs::remove_file(&resolved).expect("remove");
    std::os::unix::fs::symlink(&secret, &resolved).expect("link");
    assert_eq!(images::read_allowed(&resolved), Err(ImageError::NotAllowed));
}

#[cfg(unix)]
#[test]
fn a_file_is_checked_again_at_fetch_time() {
    let home = Scratch::new("data-file");
    let tmp = TmpFolder::new("fetch");
    let path = scratch_file(&tmp.0, "note.png", &png(64, 64, 100));
    let (_, refs) = images::file_markers(&format!("[Image: source: {path}]"), &home.dir);
    let transcript = home.path("t.jsonl");
    assert_eq!(
        images::data(&refs[0], &transcript, &home.dir).map(|d| d.len()),
        Ok(100)
    );
    #[cfg(unix)]
    {
        std::fs::remove_file(&path).expect("remove");
        std::os::unix::fs::symlink("/etc/hosts", &path).expect("link");
        assert_eq!(
            images::data(&refs[0], &transcript, &home.dir),
            Err(ImageError::NotAllowed)
        );
    }
}

#[test]
fn pixel_sizes_come_from_the_header_of_each_format() {
    assert_eq!(images::pixel_size(&png(30, 65, 64)), Some((30, 65)));
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&[0x40, 0x01, 0xC8, 0x00, 0, 0, 0]);
    assert_eq!(images::pixel_size(&gif), Some((320, 200)));
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0xAA, 0xBB];
    jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 120, 0x00, 80, 3]);
    assert_eq!(images::pixel_size(&jpeg), Some((80, 120)));
    let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
    webp.extend_from_slice(&[
        0, 0, 0, 0, 0, 0, 0, 0x9D, 0x01, 0x2A, 0x20, 0x01, 0x10, 0x00,
    ]);
    assert_eq!(images::pixel_size(&webp), Some((288, 16)));
    assert_eq!(images::pixel_size(b"not an image"), None);
    assert_eq!(images::pixel_size(&PNG_SIGNATURE), None, "a cut header");
}
