//! Pictures in the phone's chat view (ConsoleCore's `ChatImages`). A chat message only ever
//! carries a description of each picture ([`ImageRef`]); the bytes are read on demand, from the
//! transcript line the picture sits in or from a file the message names, and the bridge sends them
//! as a downscaled JPEG (`bridge::images`).

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const ALLOWED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "heic"];
/// A line holding a picture is read whole up to this size.
const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOrigin {
    User,
    ToolResult,
    File,
}

/// A picture a chat message refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// Stable within one transcript: where the picture lives (`b-<line offset>-<block>` or
    /// `f-<hash of the path>`).
    pub id: String,
    pub media_type: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub origin: ImageOrigin,
    /// Size of the original, for the viewer.
    pub byte_count: i64,
    /// For a transcript block: the line's offset in the file and the picture's index among the line's.
    pub line_offset: Option<u64>,
    pub block_index: Option<usize>,
    /// For a file named in the text.
    pub path: Option<String>,
}

impl ImageRef {
    /// A user picture at block `index` of the line at `offset`, with no size known.
    pub fn for_block(offset: u64, index: usize, media_type: &str) -> ImageRef {
        ImageRef {
            id: format!("b-{offset}-{index}"),
            media_type: media_type.to_string(),
            width: None,
            height: None,
            origin: ImageOrigin::User,
            byte_count: 0,
            line_offset: Some(offset),
            block_index: Some(index),
            path: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    NotFound,
    NotAllowed,
}

/// What a line's picture was before its base64 was cut out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stripped {
    pub base64_length: usize,
    /// Read from the head of a PNG; the other formats keep theirs in places a 64-character head misses.
    pub size: Option<(i64, i64)>,
}

const BASE64_MARKER: &str = "\"type\":\"base64\"";
const MEDIA_MARKER: &[u8] = b"\"type\":\"base64\",\"media_type\":\"";
const DATA_KEY: &[u8] = b"\",\"data\":\"";
const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Cuts the pictures' base64 out of a line before the JSON is read, so the chat parser never holds a
/// picture in memory. Returns the line without the data (each `data` string becomes empty) and the
/// pictures' sizes in the order they appear.
pub fn strip_base64_blocks(line: &str) -> (Cow<'_, str>, Vec<Stripped>) {
    if !line.contains(BASE64_MARKER) {
        return (Cow::Borrowed(line), Vec::new());
    }
    let bytes = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len().min(200_000));
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let Some(found) = find(MEDIA_MARKER, bytes, index) else {
            break;
        };
        // The media type runs to its closing quote, then comes `","data":"`.
        let after_marker = found + MEDIA_MARKER.len();
        let mut cursor = after_marker;
        while cursor < bytes.len() && bytes[cursor] != b'"' {
            cursor += 1;
        }
        if !bytes[cursor.min(bytes.len())..].starts_with(DATA_KEY) {
            out.extend_from_slice(&bytes[index..after_marker]);
            index = after_marker;
            continue;
        }
        let data_start = cursor + DATA_KEY.len();
        let mut data_end = data_start;
        while data_end < bytes.len() && bytes[data_end] != b'"' {
            data_end += 1;
        }
        out.extend_from_slice(&bytes[index..data_start]);
        let head_end = data_end.min(data_start + 64);
        blocks.push(Stripped {
            base64_length: data_end - data_start,
            size: png_size_of_base64_head(&bytes[data_start..head_end]),
        });
        index = data_end;
    }
    out.extend_from_slice(&bytes[index..]);
    // Every cut sits on an ASCII quote, so what remains is still valid UTF-8.
    (
        Cow::Owned(String::from_utf8_lossy(&out).into_owned()),
        blocks,
    )
}

fn find(needle: &[u8], haystack: &[u8], from: usize) -> Option<usize> {
    if haystack.len() < needle.len() || from > haystack.len() - needle.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

/// Width and height from the start of a PNG (the IHDR chunk is in the first 24 bytes), when that is
/// what the base64 head holds.
fn png_size_of_base64_head(head: &[u8]) -> Option<(i64, i64)> {
    let usable = head.len().min(44) / 4 * 4;
    let data = base64::engine::general_purpose::STANDARD
        .decode(&head[..usable])
        .ok()?;
    png_size(&data)
}

fn png_size(data: &[u8]) -> Option<(i64, i64)> {
    if data.len() < 24 || data[..8] != PNG_SIGNATURE {
        return None;
    }
    let int = |at: usize| {
        i64::from(u32::from_be_bytes([
            data[at],
            data[at + 1],
            data[at + 2],
            data[at + 3],
        ]))
    };
    Some((int(16), int(20)))
}

/// The picture blocks of a message's content in the order they appear (also inside tool results),
/// with whether each one is a tool's result.
fn ordered_images(content: Option<&Value>) -> Vec<(&Value, bool)> {
    fn walk<'a>(value: Option<&'a Value>, nested: bool, found: &mut Vec<(&'a Value, bool)>) {
        let Some(blocks) = value.and_then(Value::as_array) else {
            return;
        };
        for block in blocks.iter().filter(|b| b.is_object()) {
            let source_type = block
                .get("source")
                .and_then(|s| s.get("type"))
                .and_then(Value::as_str);
            match block.get("type").and_then(Value::as_str) {
                Some("image") if source_type == Some("base64") => found.push((block, nested)),
                Some("tool_result") => walk(block.get("content"), true, found),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    walk(content, false, &mut found);
    found
}

/// References for the pictures in `content` (a message's content or a queued command's prompt) of
/// the line at `line_offset`, each with whether it is a tool's result. `stripped` are the sizes the
/// line's base64 had before it was cut out.
pub fn refs(
    content: Option<&Value>,
    line_offset: u64,
    stripped: &[Stripped],
) -> Vec<(ImageRef, bool)> {
    ordered_images(content)
        .into_iter()
        .enumerate()
        .map(|(index, (block, in_tool_result))| {
            let media_type = block
                .get("source")
                .and_then(|s| s.get("media_type"))
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            let info = stripped.get(index);
            let mut image = ImageRef::for_block(line_offset, index, media_type);
            image.width = info.and_then(|i| i.size).map(|s| s.0);
            image.height = info.and_then(|i| i.size).map(|s| s.1);
            image.byte_count = info.map_or(0, |i| (i.base64_length * 3 / 4) as i64);
            if in_tool_result {
                image.origin = ImageOrigin::ToolResult;
            }
            (image, in_tool_result)
        })
        .collect()
}

// MARK: Files named in the text

/// Whether a path from a chat message may be shown: an image file that exists, under the home folder
/// or /tmp. Paths are checked after symlinks and `..` are resolved. Unix paths only: on Windows
/// nothing named in the text is shown yet.
pub fn is_allowed_file(path: &str, home: &Path) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    let Ok(resolved) = std::fs::canonicalize(path) else {
        return false;
    };
    let extension = resolved
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_EXTENSIONS.contains(&extension.as_str()) {
        return false;
    }
    let mut roots = vec![
        Path::new("/tmp").to_path_buf(),
        Path::new("/private/tmp").to_path_buf(),
    ];
    if let Ok(home) = std::fs::canonicalize(home) {
        roots.push(home);
    }
    // `Path::starts_with` compares whole components, so /tmpfoo is not under /tmp.
    roots.iter().any(|root| resolved.starts_with(root))
        && std::fs::metadata(&resolved).is_ok_and(|m| m.is_file())
}

fn marker_pattern() -> Option<&'static regex::Regex> {
    static PATTERN: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    PATTERN
        .get_or_init(|| regex::Regex::new(r"\[Image: source: ([^\]\n]+)\]").ok())
        .as_ref()
}

fn media_type_of(path: &str) -> &'static str {
    let extension = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        _ => "image/png",
    }
}

/// `[Image: source: /path/x.png]` markers in `text`: the text without the ones that point at a shown
/// file, and the references. Markers whose path is not allowed stay in the text as written.
pub fn file_markers(text: &str, home: &Path) -> (String, Vec<ImageRef>) {
    let Some(pattern) = marker_pattern().filter(|_| text.contains("[Image: source:")) else {
        return (text.trim().to_string(), Vec::new());
    };
    let mut refs: Vec<ImageRef> = Vec::new();
    let mut result = text.to_string();
    let matches: Vec<(std::ops::Range<usize>, String)> = pattern
        .captures_iter(text)
        .filter_map(|c| Some((c.get(0)?.range(), c.get(1)?.as_str().trim().to_string())))
        .collect();
    for (range, path) in matches.into_iter().rev() {
        if !is_allowed_file(&path, home) {
            continue;
        }
        let byte_count = std::fs::metadata(&path).map_or(0, |m| m.len() as i64);
        // Without the real size the phone draws a 4:3 box and crops a tall screenshot to its middle.
        let size = pixel_size_of_file(&path);
        refs.push(ImageRef {
            id: format!("f-{}", &sha256_hex(&path)[..16]),
            media_type: media_type_of(&path).to_string(),
            width: size.map(|s| s.0),
            height: size.map(|s| s.1),
            origin: ImageOrigin::File,
            byte_count,
            line_offset: None,
            block_index: None,
            path: Some(path),
        });
        result.replace_range(range, "");
    }
    refs.reverse();
    (result.trim().to_string(), refs)
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Width and height of an image file, read from its header only (the pixels are not decoded).
pub fn pixel_size_of_file(path: &str) -> Option<(i64, i64)> {
    let mut file = File::open(path).ok()?;
    let mut head = vec![0u8; 32];
    let read = file.read(&mut head).ok()?;
    head.truncate(read);
    if head.starts_with(&[0xFF, 0xD8]) {
        return jpeg_size(&mut file);
    }
    pixel_size(&head)
}

/// Width and height from the first bytes of a PNG, GIF, WebP or JPEG (a JPEG's size sits after its
/// leading segments, so give it the whole file or use [`pixel_size_of_file`]).
pub fn pixel_size(data: &[u8]) -> Option<(i64, i64)> {
    if data.starts_with(&PNG_SIGNATURE) {
        return png_size(data);
    }
    if data.starts_with(b"GIF8") && data.len() >= 10 {
        let word = |at: usize| i64::from(u16::from_le_bytes([data[at], data[at + 1]]));
        return Some((word(6), word(8)));
    }
    if data.starts_with(&[0xFF, 0xD8]) {
        return jpeg_size(&mut std::io::Cursor::new(data));
    }
    if data.len() >= 30 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return webp_size(data);
    }
    None
}

fn webp_size(data: &[u8]) -> Option<(i64, i64)> {
    let word = |at: usize| i64::from(u16::from_le_bytes([data[at], data[at + 1]]));
    let triple = |at: usize| {
        i64::from(u32::from_le_bytes([
            data[at],
            data[at + 1],
            data[at + 2],
            0,
        ]))
    };
    match &data[12..16] {
        b"VP8 " if data[23..26] == [0x9D, 0x01, 0x2A] => {
            Some((word(26) & 0x3FFF, word(28) & 0x3FFF))
        }
        b"VP8L" if data[20] == 0x2F => {
            let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
            Some((
                i64::from(bits & 0x3FFF) + 1,
                i64::from((bits >> 14) & 0x3FFF) + 1,
            ))
        }
        b"VP8X" => Some((triple(24) + 1, triple(27) + 1)),
        _ => None,
    }
}

/// Walks a JPEG's segments to its start-of-frame marker.
fn jpeg_size<R: Read + Seek>(reader: &mut R) -> Option<(i64, i64)> {
    reader.seek(SeekFrom::Start(2)).ok()?;
    loop {
        let mut marker = [0u8; 2];
        reader.read_exact(&mut marker).ok()?;
        if marker[0] != 0xFF {
            return None;
        }
        let kind = marker[1];
        if kind == 0xD8 || kind == 0x01 || (0xD0..=0xD7).contains(&kind) || kind == 0xFF {
            // No length follows these; 0xFF is fill, so look at the next byte as a marker.
            if kind == 0xFF {
                reader.seek(SeekFrom::Current(-1)).ok()?;
            }
            continue;
        }
        let mut length = [0u8; 2];
        reader.read_exact(&mut length).ok()?;
        let length = i64::from(u16::from_be_bytes(length));
        let is_frame = matches!(kind, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_frame {
            let mut frame = [0u8; 5];
            reader.read_exact(&mut frame).ok()?;
            let height = i64::from(u16::from_be_bytes([frame[1], frame[2]]));
            let width = i64::from(u16::from_be_bytes([frame[3], frame[4]]));
            return Some((width, height));
        }
        if kind == 0xD9 || kind == 0xDA || length < 2 {
            return None;
        }
        reader.seek(SeekFrom::Current(length - 2)).ok()?;
    }
}

// MARK: Reading the bytes

/// The original bytes of a picture. For a transcript block the line is read again and the block
/// decoded; a file is checked again now, since it may have changed or become a link since the chat
/// was read.
pub fn data(image: &ImageRef, transcript: &Path, home: &Path) -> Result<Vec<u8>, ImageError> {
    if let Some(path) = &image.path {
        if !is_allowed_file(path, home) {
            return Err(ImageError::NotAllowed);
        }
        return std::fs::read(path).map_err(|_| ImageError::NotFound);
    }
    let (Some(offset), Some(index)) = (image.line_offset, image.block_index) else {
        return Err(ImageError::NotFound);
    };
    let line = read_line(transcript, offset).ok_or(ImageError::NotFound)?;
    let object: Value = serde_json::from_slice(&line).map_err(|_| ImageError::NotFound)?;
    let content = object
        .get("message")
        .and_then(|m| m.get("content"))
        .or_else(|| object.get("attachment").and_then(|a| a.get("prompt")));
    let blocks = ordered_images(content);
    let base64 = blocks
        .get(index)
        .and_then(|(block, _)| block.get("source"))
        .and_then(|s| s.get("data"))
        .and_then(Value::as_str)
        .ok_or(ImageError::NotFound)?;
    base64::engine::general_purpose::STANDARD
        .decode(base64)
        .map_err(|_| ImageError::NotFound)
}

/// The line starting at `offset`, without its newline. A line is read in pieces until its newline.
fn read_line(path: &Path, offset: u64) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut line = Vec::new();
    let mut piece = vec![0u8; 1 << 20];
    while line.len() < MAX_LINE_BYTES {
        let read = file.read(&mut piece).ok()?;
        if read == 0 {
            break;
        }
        if let Some(newline) = piece[..read].iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&piece[..newline]);
            break;
        }
        line.extend_from_slice(&piece[..read]);
    }
    Some(line)
}
