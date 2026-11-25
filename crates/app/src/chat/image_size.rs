pub fn size(head: &[u8]) -> Option<(u32, u32)> {
    let le16 = |i: usize| Some(u16::from_le_bytes([*head.get(i)?, *head.get(i + 1)?]) as u32);
    let be32 = |i: usize| {
        let b = head.get(i..i + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let found = if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        (be32(16)?, be32(20)?)
    } else if head.starts_with(b"GIF8") {
        (le16(6)?, le16(8)?)
    } else if head.starts_with(&[0xFF, 0xD8]) {
        jpeg(head)?
    } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        webp(head)?
    } else {
        return None;
    };
    (found.0 > 0 && found.1 > 0).then_some(found)
}

fn jpeg(head: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < head.len() {
        if head[i] != 0xFF {
            return None;
        }
        let marker = head[i + 1];
        let length = u16::from_be_bytes([head[i + 2], head[i + 3]]) as usize;
        let frame = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if frame {
            let h = u16::from_be_bytes([head[i + 5], head[i + 6]]) as u32;
            let w = u16::from_be_bytes([head[i + 7], head[i + 8]]) as u32;
            return Some((w, h));
        }
        i += 2 + length;
    }
    None
}

fn webp(head: &[u8]) -> Option<(u32, u32)> {
    let chunk = head.get(12..16)?;
    let at = |i: usize| head.get(i).copied().map(u32::from);
    match chunk {
        b"VP8X" => {
            let w = 1 + (at(24)? | at(25)? << 8 | at(26)? << 16);
            let h = 1 + (at(27)? | at(28)? << 8 | at(29)? << 16);
            Some((w, h))
        }
        b"VP8 " => {
            let w = (at(26)? | at(27)? << 8) & 0x3FFF;
            let h = (at(28)? | at(29)? << 8) & 0x3FFF;
            Some((w, h))
        }
        b"VP8L" => {
            let b = |i: usize| at(21 + i);
            let w = 1 + (b(0)? | (b(1)? & 0x3F) << 8);
            let h = 1 + ((b(1)? >> 6) | b(2)? << 2 | (b(3)? & 0x0F) << 10);
            Some((w, h))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_says_its_size() {
        let mut head = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        head.extend(1536u32.to_be_bytes());
        head.extend(1024u32.to_be_bytes());
        assert_eq!(size(&head), Some((1536, 1024)));
    }

    #[test]
    fn a_gif_and_a_jpeg_say_theirs() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend(640u16.to_le_bytes());
        gif.extend(480u16.to_le_bytes());
        assert_eq!(size(&gif), Some((640, 480)));
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        jpeg.extend([0u8; 14]);
        jpeg.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        jpeg.extend(300u16.to_be_bytes());
        jpeg.extend(400u16.to_be_bytes());
        jpeg.extend([0u8; 8]);
        assert_eq!(size(&jpeg), Some((400, 300)));
    }

    #[test]
    fn anything_else_has_none() {
        assert_eq!(size(b"hello"), None);
        assert_eq!(size(b"\x89PNG\r\n\x1a\n"), None, "cut short");
    }
}
