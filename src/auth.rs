use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use subtle::ConstantTimeEq;

pub const COOKIE_NAME: &str = "rally_session";
pub const COOKIE_MAX_AGE_SECS: i64 = 2_592_000;

pub fn password_matches(expected: &str, given: &str) -> bool {
    expected.len() == given.len() && expected.as_bytes().ct_eq(given.as_bytes()).into()
}

pub fn session_cookie(token: &str) -> String {
    format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={COOKIE_MAX_AGE_SECS}")
}

#[derive(Default)]
pub struct SessionStore(Mutex<HashSet<String>>);

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(&self) -> String {
        let bytes: [u8; 32] = rand::random();
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(token.clone());
        token
    }

    pub fn is_valid(&self, token: &str) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).contains(token)
    }
}

#[derive(Default)]
struct Failures {
    count: u32,
    blocked_until: Option<Instant>,
}

pub struct RateLimiter {
    max_failures: u32,
    block: Duration,
    map: Mutex<HashMap<IpAddr, Failures>>,
}

impl RateLimiter {
    pub fn new(max_failures: u32, block: Duration) -> Self {
        RateLimiter { max_failures, block, map: Mutex::new(HashMap::new()) }
    }

    pub fn is_blocked(&self, ip: IpAddr, now: Instant) -> bool {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let Some(f) = map.get_mut(&ip) else { return false };
        match f.blocked_until {
            Some(until) if now < until => true,
            Some(_) => {
                map.remove(&ip);
                false
            }
            None => false,
        }
    }

    pub fn record_failure(&self, ip: IpAddr, now: Instant) {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let f = map.entry(ip).or_default();
        f.count += 1;
        if f.count >= self.max_failures {
            f.blocked_until = Some(now + self.block);
        }
    }

    pub fn record_success(&self, ip: IpAddr) {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).remove(&ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::time::{Duration, Instant};

    #[test]
    fn rate_limiter_blocks_after_five_failures_for_ten_seconds() {
        let rl = RateLimiter::new(5, Duration::from_secs(10));
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let t0 = Instant::now();
        for _ in 0..4 {
            rl.record_failure(ip, t0);
        }
        assert!(!rl.is_blocked(ip, t0));
        rl.record_failure(ip, t0);
        assert!(rl.is_blocked(ip, t0));
        assert!(rl.is_blocked(ip, t0 + Duration::from_secs(9)));
        assert!(!rl.is_blocked(ip, t0 + Duration::from_secs(10)));
        rl.record_success(ip);
        assert!(!rl.is_blocked(ip, t0));
        let other: IpAddr = "10.0.0.2".parse().unwrap();
        assert!(!rl.is_blocked(other, t0));
    }

    #[test]
    fn session_tokens_are_unique_and_validated() {
        let s = SessionStore::new();
        let a = s.issue();
        let b = s.issue();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(s.is_valid(&a));
        assert!(s.is_valid(&b));
        assert!(!s.is_valid("nope"));
    }

    #[test]
    fn password_compare() {
        assert!(password_matches("abc", "abc"));
        assert!(!password_matches("abc", "abd"));
        assert!(!password_matches("abc", "ab"));
        assert!(!password_matches("", "a"));
    }

    #[test]
    fn cookie_format() {
        assert_eq!(
            session_cookie("t0k"),
            "rally_session=t0k; HttpOnly; SameSite=Lax; Path=/; Max-Age=2592000"
        );
    }
}
