//! The wire format between a client and the session host, over a Unix socket (`HostWire.swift`).
//!
//! A frame is `[length: u32 big-endian][type: u8][payload]`, where length counts the type byte and the
//! payload. Type 1 is a JSON object. Type 2 is terminal data: a 16-byte session id followed by the
//! bytes (output going to the client, or input going to the pty).
//!
//! JSON requests: `{"id":Int,"op":String, ...}`. Replies: `{"id":Int,"ok":true,"data":{...}}` or
//! `{"id":Int,"ok":false,"error":String}`. Events: `{"event":"exit","sessionId":String,"status":Int}`
//! and `{"event":"spawned","sessionId":String}`.

use serde_json::{Map, Value};
use std::fmt;

/// The largest payload a frame may carry.
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// A session's id: a UUID, written the way Swift's `uuidString` writes it (upper case, dashed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId([u8; 16]);

impl SessionId {
    pub fn from_bytes(bytes: [u8; 16]) -> SessionId {
        SessionId(bytes)
    }

    pub fn bytes(&self) -> [u8; 16] {
        self.0
    }

    /// A random (version 4) id.
    pub fn random() -> SessionId {
        let mut bytes = random_bytes();
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        SessionId(bytes)
    }

    /// `8-4-4-4-12` hex digits, either case, as `UUID(uuidString:)` takes them.
    pub fn parse(text: &str) -> Option<SessionId> {
        let groups: Vec<&str> = text.split('-').collect();
        let lengths: Vec<usize> = groups.iter().map(|g| g.len()).collect();
        if lengths != [8, 4, 4, 4, 12] {
            return None;
        }
        let hex: String = groups.concat();
        let mut bytes = [0u8; 16];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(SessionId(bytes))
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                write!(f, "-")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

/// Sixteen bytes from the system's random source; on a system without `/dev/urandom` (not one the host
/// runs on), from the standard library's randomly seeded hasher.
fn random_bytes() -> [u8; 16] {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        if file.read_exact(&mut bytes).is_ok() {
            return bytes;
        }
    }
    use std::hash::{BuildHasher, Hasher};
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_usize(i);
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        chunk.copy_from_slice(&hasher.finish().to_be_bytes());
    }
    bytes
}

/// One frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Json(Map<String, Value>),
    Data { session: SessionId, bytes: Vec<u8> },
}

/// `value` with every object's keys in sorted order. The workspace builds serde_json with
/// `preserve_order` (through GPUI), so a Map keeps insertion order unless it is rebuilt sorted.
pub fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for key in keys {
                if let Some(v) = object.get(key) {
                    out.insert(key.clone(), sorted(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The bytes for `frame`. JSON keys are sorted, as Swift's `.sortedKeys` writes them.
pub fn encode(frame: &Frame) -> Vec<u8> {
    let mut payload = Vec::new();
    match frame {
        Frame::Json(object) => {
            payload.push(1);
            let value = sorted(&Value::Object(object.clone()));
            // A Value always serializes; "{}" is the Swift fallback all the same.
            let text = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
            payload.extend_from_slice(&text);
        }
        Frame::Data { session, bytes } => {
            payload.push(2);
            payload.extend_from_slice(&session.bytes());
            payload.extend_from_slice(bytes);
        }
    }
    let length = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    let mut out = length.to_be_bytes().to_vec();
    out.extend_from_slice(&payload);
    out
}

/// A JSON frame.
pub fn json(object: Map<String, Value>) -> Vec<u8> {
    encode(&Frame::Json(object))
}

/// Why a stream of frames ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadFailure {
    TooLarge,
    Malformed,
}

/// Collects bytes from a socket and yields whole frames. A frame that is too big or malformed ends the
/// stream: nothing more is read.
#[derive(Debug, Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
    failure: Option<ReadFailure>,
}

impl FrameReader {
    pub fn new() -> FrameReader {
        FrameReader::default()
    }

    pub fn failure(&self) -> Option<ReadFailure> {
        self.failure
    }

    pub fn feed(&mut self, data: &[u8]) -> Vec<Frame> {
        if self.failure.is_some() {
            return Vec::new();
        }
        self.buffer.extend_from_slice(data);
        let mut frames = Vec::new();
        while self.buffer.len() >= 5 {
            let length = u32::from_be_bytes([
                self.buffer[0],
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
            ]) as usize;
            if length == 0 {
                self.fail(ReadFailure::Malformed);
                break;
            }
            if length > MAX_PAYLOAD {
                self.fail(ReadFailure::TooLarge);
                break;
            }
            if self.buffer.len() < 4 + length {
                break;
            }
            let kind = self.buffer[4];
            let payload: Vec<u8> = self.buffer[5..4 + length].to_vec();
            self.buffer.drain(..4 + length);
            match kind {
                1 => match serde_json::from_slice::<Value>(&payload) {
                    Ok(Value::Object(object)) => frames.push(Frame::Json(object)),
                    _ => {
                        self.fail(ReadFailure::Malformed);
                        return frames;
                    }
                },
                2 if payload.len() >= 16 => {
                    let mut id = [0u8; 16];
                    id.copy_from_slice(&payload[..16]);
                    frames.push(Frame::Data {
                        session: SessionId(id),
                        bytes: payload[16..].to_vec(),
                    });
                }
                _ => {
                    self.fail(ReadFailure::Malformed);
                    return frames;
                }
            }
        }
        frames
    }

    fn fail(&mut self, failure: ReadFailure) {
        self.failure = Some(failure);
        self.buffer.clear();
    }
}

/// A JSON number as an integer, as Swift's `HostJSON.int` reads one (a fraction is cut off).
pub fn int(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|v| i64::try_from(v).unwrap_or(i64::MAX)))
        .or_else(|| value.as_f64().map(|v| v as i64))
}
