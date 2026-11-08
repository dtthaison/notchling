//! `BridgeCrypto`: hashes, constant-time compares, random tokens and the base64 the bridge uses.

use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};

/// The system's random source failed (never expected; reported, not panicked on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomFailed;

impl std::fmt::Display for RandomFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the system's random source failed")
    }
}

impl std::error::Error for RandomFailed {}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(digest::digest(&digest::SHA256, data).as_ref());
    out
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xF) as usize] as char);
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256(data))
}

/// Hex text to bytes, as Swift's `DeviceStore.hexBytes`: pairs of digits, a bad pair is 0, an odd
/// last digit is dropped.
pub fn hex_bytes(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|p| u8::from_str_radix(p, 16).ok())
                .unwrap_or(0)
        })
        .collect()
}

/// Compares without stopping at the first difference, so timing does not leak how much matched.
pub fn constant_time_eq(lhs: &[u8], rhs: &[u8]) -> bool {
    if lhs.len() != rhs.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in lhs.iter().zip(rhs) {
        difference |= a ^ b;
    }
    std::hint::black_box(difference) == 0
}

pub fn random_bytes(count: usize) -> Result<Vec<u8>, RandomFailed> {
    let mut bytes = vec![0u8; count];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| RandomFailed)?;
    Ok(bytes)
}

/// `bytes` random bytes as base64url without padding (32 bytes: 43 characters).
pub fn random_token(bytes: usize) -> Result<String, RandomFailed> {
    Ok(base64url(&random_bytes(bytes)?))
}

/// A random (version 4) UUID in Foundation's uppercase form, as `UUID().uuidString`.
pub fn uuid_v4() -> Result<String, RandomFailed> {
    let mut b = random_bytes(16)?;
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    let h = hex(&b).to_uppercase();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(STD[((n >> (18 - 6 * i)) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// base64url without padding.
pub fn base64url(data: &[u8]) -> String {
    base64(data)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

/// Decodes standard or url-safe base64, padded or not; `None` for anything else.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let body = text.trim_end_matches('=');
    if text.len() - body.len() > 2 || body.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in body.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_shape_hash_and_constant_time_compare() {
        let token = random_token(32).unwrap();
        assert_eq!(token.len(), 43, "32 bytes, base64url, no padding");
        assert!(!token.contains(['+', '/', '=']));
        assert_ne!(token, random_token(32).unwrap());
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"sama"));
        assert!(!constant_time_eq(b"same", b"samee"), "different length");
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn base64_round_trips_both_alphabets() {
        for (raw, std) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"\xfb\xff", "+/8="),
        ] {
            assert_eq!(base64(raw), std);
            assert_eq!(base64_decode(std).unwrap(), raw);
            assert_eq!(base64_decode(&base64url(raw)).unwrap(), raw);
        }
        assert_eq!(base64url(b"\xfb\xff"), "-_8");
        assert_eq!(base64_decode("a"), None);
        assert_eq!(base64_decode("ab!c"), None);
    }

    #[test]
    fn uuids_look_like_foundations() {
        let id = uuid_v4().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(id, id.to_uppercase());
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, uuid_v4().unwrap());
    }

    #[test]
    fn hex_bytes_reads_pairs() {
        assert_eq!(hex_bytes("00ff10"), vec![0, 255, 16]);
        assert_eq!(hex_bytes("zz1"), vec![0]);
    }
}
