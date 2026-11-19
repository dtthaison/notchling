use super::model::ShotSpot;
use crate::bridge::images::{ImageCrate, Imaging};
use base64::Engine as _;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

pub const LINE_CAP: u64 = 8 * 1024 * 1024;
pub const THUMB_SIDE: i64 = 640;

const IMAGE_BLOCK: &[u8] = br#""type":"image""#;
const DATA_KEY: &[u8] = br#""data":""#;

pub fn read_line(path: &str, at: u64, cap: u64) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(at)).ok()?;
    let mut line = Vec::new();
    BufReader::new(file.take(cap + 1))
        .read_until(b'\n', &mut line)
        .ok()?;
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    (line.len() as u64 <= cap).then_some(line)
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() || haystack.len() - from < needle.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

pub fn image_data<'a>(line: &'a [u8], id: &str, index: u32) -> Option<&'a [u8]> {
    let key = format!(r#""tool_use_id":"{id}""#);
    find(line, key.as_bytes(), 0)?;
    let mut from = 0;
    for _ in 0..=index {
        from = find(line, IMAGE_BLOCK, from)? + IMAGE_BLOCK.len();
    }
    let start = find(line, DATA_KEY, from)? + DATA_KEY.len();
    let len = line[start..].iter().position(|&b| b == b'"')?;
    Some(&line[start..start + len])
}

pub fn thumbnail(spot: &ShotSpot, id: &str) -> Option<Vec<u8>> {
    let line = read_line(&spot.path, spot.at, LINE_CAP)?;
    let data = image_data(&line, id, spot.index)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    drop(line);
    ImageCrate.jpeg(&bytes, THUMB_SIDE).ok()
}

#[cfg(test)]
#[path = "browser_shot_tests.rs"]
mod tests;
