use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;

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
pub const MAX_ENTRIES: usize = 4096;
pub const LOCK_MS: i64 = 5 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Blocked(i64),
}

#[derive(Clone, Copy, Default)]
struct Entry {
    failures: u32,
    blocked_until: Option<i64>,
    last_failure: i64,
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
        match self.lock().get(&key(address)).and_then(|e| e.blocked_until) {
            Some(until) if until > now_ms => Verdict::Blocked(until),
            _ => Verdict::Allowed,
        }
    }

    pub fn record_failure(&self, address: &str, now_ms: i64) -> Verdict {
        let address = key(address);
        let mut entries = self.lock();
        if entries.len() >= MAX_ENTRIES && !entries.contains_key(&address) {
            let oldest = entries
                .iter()
                .min_by_key(|(_, e)| e.last_failure)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                entries.remove(&oldest);
            }
        }
        let mut entry = entries.get(&address).copied().unwrap_or_default();
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
        entry.last_failure = now_ms;
        entries.insert(address, entry);
        Verdict::Blocked(until)
    }

    pub fn record_success(&self, address: &str) {
        self.lock().remove(&key(address));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
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
    fn an_ipv6_host_is_counted_by_its_64_and_a_mapped_ipv4_as_itself() {
        let limiter = AuthRateLimiter::new();
        limiter.record_failure("2001:db8:1:2::1", NOW);
        assert!(matches!(
            limiter.check("2001:db8:1:2:ffff::99", NOW),
            Verdict::Blocked(_)
        ));
        assert_eq!(limiter.check("2001:db8:1:3::1", NOW), Verdict::Allowed);
        limiter.record_failure("::ffff:192.168.1.9", NOW);
        assert!(matches!(
            limiter.check("192.168.1.9", NOW),
            Verdict::Blocked(_)
        ));
        limiter.record_success("2001:db8:1:2::abcd");
        assert_eq!(limiter.check("2001:db8:1:2::1", NOW), Verdict::Allowed);
    }

    #[test]
    fn the_map_is_capped_and_the_oldest_goes_first() {
        let limiter = AuthRateLimiter::new();
        let address = |n: usize| format!("10.{}.{}.{}", n >> 16, (n >> 8) & 0xFF, n & 0xFF);
        let total = MAX_ENTRIES + 10;
        for n in 0..total {
            limiter.record_failure(&address(n), NOW + n as i64);
        }
        assert_eq!(limiter.len(), MAX_ENTRIES);
        let late = NOW + total as i64;
        assert_eq!(
            limiter.check(&address(0), late),
            Verdict::Allowed,
            "evicted"
        );
        assert!(matches!(
            limiter.check(&address(total - 1), late),
            Verdict::Blocked(_)
        ));
    }

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
