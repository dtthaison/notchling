//! `AuthRateLimiter`: per-address backoff for failed logins, 1, 2, 4, 8 seconds, then a 5-minute lock
//! on the 5th failure. A tunnel client is keyed by its own address (`Cf-Connecting-Ip`), never by
//! the loopback cloudflared connects from.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;

/// What an address is counted under: itself, or for IPv6 its /64, since one host may hold a whole
/// /64 and pick a new address per try. Text that isn't an address is its own key.
pub fn key(address: &str) -> String {
    match address.parse::<IpAddr>().map(|ip| ip.to_canonical()) {
        Ok(IpAddr::V6(v6)) => {
            let s = v6.segments();
            format!("{}/64", Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
        Ok(ip) => ip.to_string(),
        Err(_) => address.to_string(),
    }
}

pub const MAX_FAILURES: u32 = 5;
pub const LOCK_MS: i64 = 5 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// Blocked until this time (milliseconds since the epoch).
    Blocked(i64),
}

#[derive(Clone, Copy, Default)]
struct Entry {
    failures: u32,
    blocked_until: Option<i64>,
}

#[derive(Default)]
pub struct AuthRateLimiter {
    entries: Mutex<HashMap<String, Entry>>,
}

impl AuthRateLimiter {
    pub fn new() -> AuthRateLimiter {
        AuthRateLimiter::default()
    }

    pub fn check(&self, address: &str, now_ms: i64) -> Verdict {
        match self.lock().get(address).and_then(|e| e.blocked_until) {
            Some(until) if until > now_ms => Verdict::Blocked(until),
            _ => Verdict::Allowed,
        }
    }

    pub fn record_failure(&self, address: &str, now_ms: i64) -> Verdict {
        let mut entries = self.lock();
        let mut entry = entries.get(address).copied().unwrap_or_default();
        // A lock that has run out starts counting from zero again.
        if entry.failures >= MAX_FAILURES && entry.blocked_until.is_some_and(|u| u <= now_ms) {
            entry = Entry::default();
        }
        entry.failures += 1;
        let delay = if entry.failures >= MAX_FAILURES {
            LOCK_MS
        } else {
            1000i64 << (entry.failures - 1)
        };
        let until = now_ms + delay;
        entry.blocked_until = Some(until);
        entries.insert(address.to_string(), entry);
        Verdict::Blocked(until)
    }

    pub fn record_success(&self, address: &str) {
        self.lock().remove(address);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn backoff_then_a_five_minute_lock() {
        let limiter = AuthRateLimiter::new();
        let ip = "192.168.1.9";
        assert_eq!(limiter.check(ip, NOW), Verdict::Allowed);
        for (index, delay) in [1000, 2000, 4000, 8000].into_iter().enumerate() {
            let at = NOW + index as i64 * 100_000;
            assert_eq!(
                limiter.record_failure(ip, at),
                Verdict::Blocked(at + delay),
                "failure {}",
                index + 1
            );
        }
        let fifth = NOW + 500_000;
        assert_eq!(
            limiter.record_failure(ip, fifth),
            Verdict::Blocked(fifth + 300_000)
        );
        assert_eq!(
            limiter.check(ip, fifth + 299_000),
            Verdict::Blocked(fifth + 300_000)
        );
        assert_eq!(limiter.check(ip, fifth + 301_000), Verdict::Allowed);
        assert_eq!(
            limiter.check("192.168.1.10", fifth),
            Verdict::Allowed,
            "per address"
        );

        limiter.record_success(ip);
        assert_eq!(
            limiter.record_failure(ip, fifth + 400_000),
            Verdict::Blocked(fifth + 401_000),
            "a success starts over"
        );
    }

    #[test]
    fn a_finished_lock_counts_from_zero() {
        let limiter = AuthRateLimiter::new();
        for i in 0..5 {
            limiter.record_failure("a", NOW + i);
        }
        let later = NOW + LOCK_MS + 10;
        assert_eq!(
            limiter.record_failure("a", later),
            Verdict::Blocked(later + 1000)
        );
    }
}
