//! Simple in-memory per-IP token bucket applied to `/api/auth/**`, `/api/public/**` POSTs and
//! `/api/demo/login`. Durable brute-force protection lives in `auth::throttle` (login_attempts table).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Token bucket per client key (usually the client IP).
#[derive(Debug)]
pub struct RateLimiter {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl RateLimiter {
    /// `capacity` requests in a burst, refilling `refill_per_sec` tokens per second.
    pub fn new(capacity: u32, refill_per_sec: f64) -> Self {
        RateLimiter { capacity: capacity as f64, refill_per_sec, buckets: Mutex::new(HashMap::new()) }
    }

    /// Takes one token for `key`; returns false when the bucket is empty.
    pub fn check(&self, key: &str) -> bool {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: &str, now: Instant) -> bool {
        let mut buckets = self.buckets.lock().expect("rate limiter mutex");
        if buckets.len() > 10_000 {
            // Drop full buckets to bound memory.
            let cap = self.capacity;
            let rate = self.refill_per_sec;
            buckets.retain(|_, (tokens, at)| (*tokens + now.duration_since(*at).as_secs_f64() * rate) < cap);
        }
        let entry = buckets.entry(key.to_string()).or_insert((self.capacity, now));
        let elapsed = now.duration_since(entry.1).as_secs_f64();
        entry.0 = (entry.0 + elapsed * self.refill_per_sec).min(self.capacity);
        entry.1 = now;
        if entry.0 >= 1.0 {
            entry.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

impl Default for RateLimiter {
    /// 30-request burst, one new request every 2 seconds.
    fn default() -> Self {
        RateLimiter::new(30, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bucket_empties_and_refills() {
        let rl = RateLimiter::new(2, 1.0);
        let t0 = Instant::now();
        assert!(rl.check_at("a", t0));
        assert!(rl.check_at("a", t0));
        assert!(!rl.check_at("a", t0));
        assert!(rl.check_at("b", t0));
        assert!(rl.check_at("a", t0 + Duration::from_millis(1100)));
    }
}
