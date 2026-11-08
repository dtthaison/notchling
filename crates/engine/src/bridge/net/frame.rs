//! `WebSocketWire`, the frame half (RFC 6455): what the server writes (final, unmasked frames) and
//! a decoder for what clients send. Clients must mask; anything off the rules ends in a
//! [`Event::Failure`] with the close code to answer with, and the decoder takes nothing more.

/// The largest message a phone may send: a frame of 256 KB plus one byte is refused (1009).
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

pub const TEXT: u8 = 0x1;
pub const BINARY: u8 = 0x2;
pub const CLOSE: u8 = 0x8;
pub const PING: u8 = 0x9;
pub const PONG: u8 = 0xA;

/// A final, unmasked frame (servers never mask).
pub fn encode(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 10);
    frame.push(0x80 | opcode);
    match payload.len() {
        n if n < 126 => frame.push(n as u8),
        n if n < 65_536 => {
            frame.push(126);
            frame.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            frame.push(127);
            frame.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(payload);
    frame
}

pub fn close_frame(code: u16) -> Vec<u8> {
    encode(CLOSE, &code.to_be_bytes())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<u16>),
    Failure(u16, String),
}

pub struct FrameDecoder {
    max_message_bytes: usize,
    buffer: Vec<u8>,
    fragment_opcode: Option<u8>,
    fragments: Vec<u8>,
    failed: bool,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        FrameDecoder::new(MAX_MESSAGE_BYTES)
    }
}

impl FrameDecoder {
    pub fn new(max_message_bytes: usize) -> FrameDecoder {
        FrameDecoder {
            max_message_bytes,
            buffer: Vec::new(),
            fragment_opcode: None,
            fragments: Vec::new(),
            failed: false,
        }
    }

    pub fn feed(&mut self, data: &[u8]) -> Vec<Event> {
        if self.failed {
            return Vec::new();
        }
        self.buffer.extend_from_slice(data);
        let mut events = Vec::new();
        while let Some(event) = self.next() {
            let failed = matches!(event, Event::Failure(..));
            events.push(event);
            if failed {
                self.failed = true;
                self.buffer = Vec::new();
                break;
            }
        }
        events
    }

    fn next(&mut self) -> Option<Event> {
        let fail = |code: u16, reason: &str| Some(Event::Failure(code, reason.into()));
        loop {
            if self.buffer.len() < 2 {
                return None;
            }
            let (first, second) = (self.buffer[0], self.buffer[1]);
            let fin = first & 0x80 != 0;
            let opcode = first & 0x0F;
            if first & 0x70 != 0 {
                return fail(1002, "reserved bits set");
            }
            if second & 0x80 == 0 {
                return fail(1002, "client frame not masked");
            }
            let mut length = (second & 0x7F) as usize;
            let mut offset = 2;
            if length == 126 {
                if self.buffer.len() < 4 {
                    return None;
                }
                length = usize::from(u16::from_be_bytes([self.buffer[2], self.buffer[3]]));
                offset = 4;
            } else if length == 127 {
                if self.buffer.len() < 10 {
                    return None;
                }
                let mut wide = [0u8; 8];
                wide.copy_from_slice(&self.buffer[2..10]);
                let wide = u64::from_be_bytes(wide);
                if wide >> 63 != 0 {
                    return fail(1002, "bad length");
                }
                if wide > self.max_message_bytes as u64 {
                    return fail(1009, "message too big");
                }
                length = wide as usize;
                offset = 10;
            }
            // Refused on the header alone: a too-big frame is never buffered.
            if length > self.max_message_bytes {
                return fail(1009, "message too big");
            }
            if self.buffer.len() < offset + 4 + length {
                return None;
            }
            let mask = [
                self.buffer[offset],
                self.buffer[offset + 1],
                self.buffer[offset + 2],
                self.buffer[offset + 3],
            ];
            let start = offset + 4;
            let payload: Vec<u8> = self.buffer[start..start + length]
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ mask[i % 4])
                .collect();
            self.buffer.drain(..start + length);

            if opcode >= 0x8 {
                if !fin || length > 125 {
                    return fail(1002, "bad control frame");
                }
                return match opcode {
                    CLOSE if length == 1 => fail(1002, "bad close payload"),
                    CLOSE => Some(Event::Close(
                        (length >= 2).then(|| u16::from_be_bytes([payload[0], payload[1]])),
                    )),
                    PING => Some(Event::Ping(payload)),
                    PONG => Some(Event::Pong(payload)),
                    _ => fail(1002, "unknown control opcode"),
                };
            }
            match opcode {
                0x0 if self.fragment_opcode.is_none() => {
                    return fail(1002, "unexpected continuation")
                }
                0x0 => {}
                TEXT | BINARY if self.fragment_opcode.is_some() => {
                    return fail(1002, "new message inside a fragmented one")
                }
                TEXT | BINARY => {
                    self.fragment_opcode = Some(opcode);
                    self.fragments = Vec::new();
                }
                _ => return fail(1002, "unknown opcode"),
            }
            if self.fragments.len() + length > self.max_message_bytes {
                return fail(1009, "message too big");
            }
            self.fragments.extend_from_slice(&payload);
            if !fin {
                continue;
            }
            let kind = self.fragment_opcode.take();
            let message = std::mem::take(&mut self.fragments);
            if kind == Some(TEXT) {
                return match String::from_utf8(message) {
                    Ok(text) => Some(Event::Text(text)),
                    Err(_) => fail(1007, "text is not UTF-8"),
                };
            }
            return Some(Event::Binary(message));
        }
    }
}

/// A masked client frame (tests and test clients).
#[cfg(test)]
pub(crate) fn masked(opcode: u8, payload: &[u8], fin: bool) -> Vec<u8> {
    let mask = [1u8, 2, 3, 4];
    let mut frame = vec![if fin { 0x80 } else { 0 } | opcode];
    match payload.len() {
        n if n < 126 => frame.push(0x80 | n as u8),
        n if n < 65_536 => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_masked_text_and_binary_with_every_length_form() {
        let mut decoder = FrameDecoder::new(300_000);
        assert_eq!(
            decoder.feed(&masked(TEXT, "xin chào".as_bytes(), true)),
            vec![Event::Text("xin chào".into())]
        );
        let medium = vec![0x41u8; 300];
        assert_eq!(
            decoder.feed(&masked(BINARY, &medium, true)),
            vec![Event::Binary(medium)]
        );
        let large = vec![0x42u8; 70_000];
        assert_eq!(
            decoder.feed(&masked(BINARY, &large, true)),
            vec![Event::Binary(large)]
        );
    }

    #[test]
    fn frames_arriving_byte_by_byte_are_reassembled() {
        let mut decoder = FrameDecoder::default();
        let mut events = Vec::new();
        for byte in masked(TEXT, b"hello", true) {
            events.extend(decoder.feed(&[byte]));
        }
        assert_eq!(events, vec![Event::Text("hello".into())]);
    }

    #[test]
    fn fragmented_messages_with_a_ping_in_the_middle() {
        let mut decoder = FrameDecoder::default();
        let mut events = decoder.feed(&masked(TEXT, b"hel", false));
        events.extend(decoder.feed(&masked(PING, b"p", true)));
        events.extend(decoder.feed(&masked(0, b"lo ", false)));
        events.extend(decoder.feed(&masked(0, b"there", true)));
        assert_eq!(
            events,
            vec![
                Event::Ping(b"p".to_vec()),
                Event::Text("hello there".into())
            ]
        );
    }

    fn failure(data: &[u8], max: usize) -> Option<u16> {
        FrameDecoder::new(max)
            .feed(data)
            .into_iter()
            .find_map(|e| match e {
                Event::Failure(code, _) => Some(code),
                _ => None,
            })
    }

    #[test]
    fn protocol_violations_end_with_the_right_close_code() {
        assert_eq!(failure(&[0x81, 0x01, 0x41], 1000), Some(1002), "unmasked");
        assert_eq!(
            failure(&masked(0, b"x", true), 1000),
            Some(1002),
            "continuation with nothing"
        );
        assert_eq!(
            failure(&masked(PING, b"x", false), 1000),
            Some(1002),
            "fragmented control"
        );
        assert_eq!(
            failure(&masked(3, b"", true), 1000),
            Some(1002),
            "reserved opcode"
        );
        assert_eq!(
            failure(&[0xC1, 0x80, 0, 0, 0, 0], 1000),
            Some(1002),
            "RSV bit"
        );
        assert_eq!(
            failure(&masked(TEXT, &[0xFF, 0xFE], true), 1000),
            Some(1007),
            "not UTF-8"
        );
        assert_eq!(
            failure(&masked(BINARY, &[0; 2000], true), 1000),
            Some(1009),
            "too big"
        );
        let mut top_bit = vec![0x82, 0xFF];
        top_bit.extend_from_slice(&[0xFF; 8]);
        top_bit.extend_from_slice(&[0; 4]);
        assert_eq!(
            failure(&top_bit, 1000),
            Some(1002),
            "64-bit length with the top bit set"
        );
        assert_eq!(
            failure(&masked(CLOSE, b"x", true), 1000),
            Some(1002),
            "one-byte close"
        );
        let mut decoder = FrameDecoder::default();
        decoder.feed(&[0x81, 0x01, 0x41]);
        assert!(
            decoder.feed(&masked(TEXT, b"x", true)).is_empty(),
            "nothing after a failure"
        );
    }

    #[test]
    fn a_256_kb_plus_one_frame_is_refused_from_its_header() {
        let mut decoder = FrameDecoder::default();
        let exact = vec![b'a'; MAX_MESSAGE_BYTES];
        assert!(matches!(
            decoder.feed(&masked(TEXT, &exact, true))[..],
            [Event::Text(_)]
        ));
        let over = masked(TEXT, &vec![b'a'; MAX_MESSAGE_BYTES + 1], true);
        let mut decoder = FrameDecoder::default();
        assert_eq!(
            failure(&over[..14], MAX_MESSAGE_BYTES),
            Some(1009),
            "the header alone is enough"
        );
        assert!(matches!(decoder.feed(&over)[..], [Event::Failure(1009, _)]));
        let mut split = FrameDecoder::default();
        let half = vec![b'a'; MAX_MESSAGE_BYTES / 2 + 1];
        split.feed(&masked(TEXT, &half, false));
        assert!(
            matches!(
                split.feed(&masked(0, &half, true))[..],
                [Event::Failure(1009, _)]
            ),
            "fragments add up"
        );
    }

    #[test]
    fn a_close_frame_carries_the_code() {
        let mut decoder = FrameDecoder::default();
        assert_eq!(
            decoder.feed(&masked(CLOSE, &[0x03, 0xE8], true)),
            vec![Event::Close(Some(1000))]
        );
    }

    #[test]
    fn server_frames_are_unmasked_with_the_right_length_forms() {
        assert_eq!(encode(TEXT, b"hi"), vec![0x81, 2, 0x68, 0x69]);
        assert_eq!(encode(BINARY, &[0; 300])[..4], [0x82, 126, 0x01, 0x2C]);
        assert_eq!(
            encode(BINARY, &[0; 70_000])[..10],
            [0x82, 127, 0, 0, 0, 0, 0, 1, 0x11, 0x70]
        );
        assert_eq!(close_frame(1002), vec![0x88, 2, 0x03, 0xEA]);
    }
}
