//! `PairingCodes`: one live pairing code at a time, 8 characters, valid 5 minutes, used once.

use super::crypto::{constant_time_eq, random_bytes, RandomFailed};
use std::sync::Mutex;

/// How long a code lives, in milliseconds.
pub const LIFETIME_MS: i64 = 5 * 60 * 1000;

/// No 0/O or 1/I/L, which look alike on a phone screen.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingCode {
    pub code: String,
    /// Milliseconds since the epoch.
    pub expires_at: i64,
}

#[derive(Default)]
pub struct PairingCodes {
    current: Mutex<Option<PairingCode>>,
}

impl PairingCodes {
    pub fn new() -> PairingCodes {
        PairingCodes::default()
    }

    /// A new code, replacing any live one.
    pub fn issue(&self, now_ms: i64) -> Result<PairingCode, RandomFailed> {
        let code: String = random_bytes(8)?
            .iter()
            .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
            .collect();
        let issued = PairingCode {
            code,
            expires_at: now_ms + LIFETIME_MS,
        };
        *self.lock() = Some(issued.clone());
        Ok(issued)
    }

    /// True once, for the live code, before it expires. A wrong guess leaves the live code alone.
    pub fn consume(&self, candidate: &str, now_ms: i64) -> bool {
        let mut current = self.lock();
        let Some(live) = current.as_ref().filter(|l| now_ms < l.expires_at) else {
            *current = None;
            return false;
        };
        let normalized = candidate.trim_matches([' ', '\t']).to_uppercase();
        if !constant_time_eq(normalized.as_bytes(), live.code.as_bytes()) {
            return false;
        }
        *current = None;
        true
    }

    pub fn live(&self, now_ms: i64) -> Option<PairingCode> {
        self.lock()
            .as_ref()
            .filter(|l| now_ms < l.expires_at)
            .cloned()
    }

    pub fn cancel(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<PairingCode>> {
        self.current.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn shape_expiry_and_one_time_use() {
        let codes = PairingCodes::new();
        let issued = codes.issue(NOW).unwrap();
        assert_eq!(issued.code.len(), 8);
        assert!(!issued.code.contains(['0', '1', 'O', 'I', 'L']));
        assert_eq!(issued.expires_at, NOW + 300_000);

        assert!(!codes.consume("WRONG123", NOW + 1000));
        assert!(
            codes.live(NOW + 1000).is_some(),
            "a wrong guess does not burn the live code"
        );
        assert!(
            codes.consume(&issued.code.to_lowercase(), NOW + 2000),
            "case-insensitive"
        );
        assert!(!codes.consume(&issued.code, NOW + 3000), "one time only");
    }

    #[test]
    fn a_code_expires_after_five_minutes() {
        let codes = PairingCodes::new();
        let issued = codes.issue(NOW).unwrap();
        assert!(!codes.consume(&issued.code, NOW + 300_000));
        assert!(codes.live(NOW + 300_000).is_none());

        let first = codes.issue(NOW).unwrap();
        let second = codes.issue(NOW).unwrap();
        if first.code != second.code {
            assert!(
                !codes.consume(&first.code, NOW),
                "issuing a new code replaces the old one"
            );
        }
        assert!(codes.consume(&second.code, NOW));
    }

    #[test]
    fn cancel_ends_the_live_code() {
        let codes = PairingCodes::new();
        let issued = codes.issue(NOW).unwrap();
        codes.cancel();
        assert!(!codes.consume(&issued.code, NOW));
        assert!(!codes.consume(" x ", NOW));
    }
}
