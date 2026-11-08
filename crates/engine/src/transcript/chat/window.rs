//! Reading the tail of a transcript as whole lines, and cutting the pictures' base64 out of a line
//! before its size is judged (ConsoleCore's `readWindow` and `ChatImages.stripBase64`).

use super::SourceLine;
use std::borrow::Cow;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// How far back a line that straddles the window's start is searched for: past this it stays cut.
const MAX_BACK_UP: usize = 64 * 1024 * 1024;
const BACK_UP_CHUNK: u64 = 1 << 20;

/// The last `bytes` of the file as whole lines with their offsets; empty lines are left out. A window
/// that begins inside a line backs up to where that line starts, so a long line (a screenshot, say) is
/// read whole instead of being dropped. None when the file can't be read.
pub fn read_window(path: &str, bytes: u64) -> Option<Vec<SourceLine>> {
    read_window_from(&mut File::open(path).ok()?, bytes)
}

pub(super) fn read_window_from<R: Read + Seek>(
    file: &mut R,
    bytes: u64,
) -> Option<Vec<SourceLine>> {
    let size = file.seek(SeekFrom::End(0)).ok()?;
    let start = size.saturating_sub(bytes);
    // Read one byte earlier to learn whether `start` is the beginning of a line.
    let from = start.saturating_sub(1);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;
    // The file can shrink between the size and the read.
    if data.is_empty() {
        return Some(Vec::new());
    }
    let mut base = from;
    let mut aligned = true;
    if start > 0 {
        aligned = data[0] == b'\n';
        data.remove(0);
        base += 1;
        if !aligned {
            if let Some((line_start, prefix)) = back_up_to_line_start(file, start) {
                base = line_start;
                data.splice(0..0, prefix);
                aligned = true;
            }
        }
    }
    let mut lines = Vec::new();
    let mut offset = base;
    for (i, raw) in data.split(|&b| b == b'\n').enumerate() {
        let line_offset = offset;
        offset += raw.len() as u64 + 1;
        if raw.is_empty() || (i == 0 && start > 0 && !aligned) {
            continue;
        }
        lines.push(SourceLine {
            offset: line_offset,
            text: String::from_utf8_lossy(raw).into_owned(),
        });
    }
    Some(lines)
}

/// Where the line that contains byte `start` begins, and its bytes before `start`. Reads backwards in
/// chunks; None when no line start is found within [`MAX_BACK_UP`] or the file can't be read.
fn back_up_to_line_start<R: Read + Seek>(file: &mut R, start: u64) -> Option<(u64, Vec<u8>)> {
    let mut cursor = start;
    let mut prefix: Vec<u8> = Vec::new();
    while cursor > 0 && prefix.len() < MAX_BACK_UP {
        let chunk_start = cursor.saturating_sub(BACK_UP_CHUNK);
        file.seek(SeekFrom::Start(chunk_start)).ok()?;
        let mut chunk = Vec::new();
        file.by_ref()
            .take(cursor - chunk_start)
            .read_to_end(&mut chunk)
            .ok()?;
        if chunk.is_empty() {
            return None;
        }
        match chunk.iter().rposition(|&b| b == b'\n') {
            Some(newline) => {
                chunk.drain(..=newline);
                chunk.extend_from_slice(&prefix);
                return Some((chunk_start + newline as u64 + 1, chunk));
            }
            None => {
                chunk.extend_from_slice(&prefix);
                prefix = chunk;
                if chunk_start == 0 {
                    return Some((0, prefix));
                }
            }
        }
        cursor = chunk_start;
    }
    None
}

const BASE64_MARKER: &str = "\"type\":\"base64\"";
const MEDIA_MARKER: &[u8] = b"\"type\":\"base64\",\"media_type\":\"";
const DATA_KEY: &[u8] = b"\",\"data\":\"";

/// A line with a picture in it is the usual reason a line is big. The base64 is cut out before the JSON
/// is read (the `data` string becomes empty), so the size limit judges the rest of the line.
pub fn strip_base64(line: &str) -> Cow<'_, str> {
    if !line.contains(BASE64_MARKER) {
        return Cow::Borrowed(line);
    }
    let bytes = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len().min(200_000));
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
        index = data_end;
    }
    out.extend_from_slice(&bytes[index..]);
    // Every cut sits on an ASCII quote, so what remains is still valid UTF-8.
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn a_file_that_shrank_between_the_size_and_the_read_gives_no_lines() {
        // Reports 10 000 bytes at the end, then reads nothing.
        struct Shrunk;
        impl Read for Shrunk {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Ok(0)
            }
        }
        impl Seek for Shrunk {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
                Ok(match pos {
                    SeekFrom::End(_) => 10_000,
                    SeekFrom::Start(n) => n,
                    SeekFrom::Current(_) => 0,
                })
            }
        }
        let lines = read_window_from(&mut Shrunk, 400).expect("a window, not a panic");
        assert!(lines.is_empty());
    }

    #[test]
    fn a_line_longer_than_the_back_up_chunk_is_read_whole() {
        let long = "x".repeat(BACK_UP_CHUNK as usize * 2 + 10);
        let text = format!("first\n{long}\nlast\n");
        let lines = read_window_from(&mut Cursor::new(text.into_bytes()), 20).expect("window");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, long);
        assert_eq!(lines[0].offset, 6);
        assert_eq!(lines[1].text, "last");
    }

    #[test]
    fn a_first_line_with_no_newline_before_it_backs_up_to_the_file_start() {
        let lines = read_window_from(&mut Cursor::new(b"abcdefgh\nend\n".to_vec()), 8).expect("w");
        assert_eq!(lines[0].text, "abcdefgh");
        assert_eq!(lines[0].offset, 0);
    }

    #[test]
    fn strip_base64_empties_the_data_and_keeps_the_rest() {
        let line = r#"{"a":[{"type":"base64","media_type":"image/png","data":"QUJDRA=="},{"type":"base64","media_type":"image/jpeg","data":"WFla"}],"b":"c"}"#;
        assert_eq!(
            strip_base64(line),
            r#"{"a":[{"type":"base64","media_type":"image/png","data":""},{"type":"base64","media_type":"image/jpeg","data":""}],"b":"c"}"#
        );
    }

    #[test]
    fn strip_base64_leaves_other_lines_alone() {
        assert!(matches!(strip_base64(r#"{"a":1}"#), Cow::Borrowed(_)));
        // The marker without a media type and data string is not a picture block.
        let odd = r#"{"type":"base64","media_type":"image/png","other":"x"}"#;
        assert_eq!(strip_base64(odd), odd);
        let truncated = r#"{"type":"base64","media_type":"image/pn"#;
        assert_eq!(strip_base64(truncated), truncated);
    }

    #[test]
    fn strip_base64_keeps_non_ascii_text() {
        let line = r#"{"t":"café","s":{"type":"base64","media_type":"image/png","data":"QQ=="},"u":"日本"}"#;
        assert_eq!(
            strip_base64(line),
            r#"{"t":"café","s":{"type":"base64","media_type":"image/png","data":""},"u":"日本"}"#
        );
    }
}
